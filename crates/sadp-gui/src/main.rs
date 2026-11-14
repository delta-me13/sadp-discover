//! sadp-discover 图形界面入口（三阶段 G1）。
//!
//! 只负责参数、日志、传输层绑定与窗口；扫描与渲染逻辑在 app 模块。
//! 事件契约与 TUI 完全相同（ScanEvent），因此两条前端可以并跑对照。

// Windows 上以 windows 子系统发布：双击时不再附送一个空的黑色控制台窗口。
// debug 构建保持控制台，`cargo run` 与测试照旧能看日志。
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod app;
mod browser;
mod console;
mod export;
mod fonts;
mod logging;

use std::net::Ipv6Addr;
use std::sync::Arc;
use std::time::Duration;

use clap::{Arg, ArgAction, Command};
use sadp_core::{Family, ScanConfig, Transport, tr_args};
use sadp_net::InterfaceSelector;
use sadp_net::udp::{UdpConfig, UdpTransport};

fn main() -> anyhow::Result<()> {
    // 必须在任何输出之前：clap 的 --help/--version 与日志都要走这里挂上的控制台。
    console::attach_parent_console();

    let matches = cli().get_matches();
    let verbose = matches.get_count("verbose");
    let _guard = logging::init(None, verbose, true);
    let lang = sadp_core::i18n::init(matches.get_one::<String>("lang").map(String::as_str));
    tracing::info!(?lang, version = sadp_core::VERSION, "GUI 启动");

    let cfg = scan_config(&matches);

    let local_port = matches.get_one::<u16>("local-port").copied();
    let transport = match bind_transport(&cfg, local_port) {
        Ok(transport) => transport,
        Err(err) => fatal(&tr_args("error.net", &[("message", &err.to_string())])),
    };
    // 设置面板改端口/网卡/地址族后，需要按新配置重新绑定传输层。
    let factory: app::TransportFactory =
        Arc::new(move |cfg: &ScanConfig| Ok(bind_transport(cfg, local_port)?));

    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_inner_size([1180.0, 740.0])
            .with_min_inner_size([820.0, 520.0])
            .with_title("sadp-discover"),
        ..Default::default()
    };

    let result = eframe::run_native(
        "sadp-discover",
        options,
        Box::new(move |cc| {
            Ok(Box::new(app::App::new(
                &cc.egui_ctx,
                cfg,
                transport,
                Some(factory),
            )))
        }),
    );
    // 窗口起不来时 eframe 的错误只在 stderr 里，双击启动同样没人看得到。
    if let Err(err) = result {
        fatal(&tr_args("error.window", &[("message", &err.to_string())]));
    }
    Ok(())
}

/// 致命错误的唯一出口：日志 + stderr，用户点开应用时再补一个弹窗。
///
/// 双击 .app 时 stderr 没有接收者，只 eprintln 等于「点了没反应」——
/// 用户只看到 Dock 图标闪一下，日志在 ~/Library/Application Support 里没人会去找。
fn fatal(message: &str) -> ! {
    tracing::error!(%message, "致命错误");
    eprintln!("{message}");
    if interactive_launch() {
        rfd::MessageDialog::new()
            .set_level(rfd::MessageLevel::Error)
            .set_title("sadp-discover")
            .set_description(message)
            .set_buttons(rfd::MessageButtons::Ok)
            .show();
    }
    std::process::exit(2);
}

/// 这次启动是不是「用户点开的应用」，而不是终端或脚本里跑起来的。
///
/// macOS 上用 LaunchServices 注入的 `__CFBundleIdentifier` 判断：Finder 双击与 `open`
/// 都会设置它，直接从终端执行二进制不会。**不用 isatty(stderr)**——脚本把输出重定向到
/// 日志文件时同样不是 tty，那种场景下弹窗会把自动化卡死；反过来终端里调试时也不该弹窗。
///
/// 其他平台没有这个变量，退回到「没有控制台 = 双击启动」的判断。
fn interactive_launch() -> bool {
    #[cfg(target_os = "macos")]
    {
        std::env::var_os("__CFBundleIdentifier").is_some()
    }
    #[cfg(not(target_os = "macos"))]
    {
        use std::io::IsTerminal;
        !std::io::stderr().is_terminal()
    }
}

/// 按配置绑定 UDP 传输层；初始启动与设置变更共用。
fn bind_transport(
    cfg: &ScanConfig,
    local_port: Option<u16>,
) -> Result<Arc<dyn Transport>, sadp_core::NetError> {
    let transport = UdpTransport::bind(UdpConfig {
        port: cfg.port,
        families: cfg.families.clone(),
        interfaces: cfg.interfaces.clone(),
        v6_mcast: cfg.v6_mcast,
        broadcast: cfg.broadcast,
        bind_v4: None,
        bind_port: local_port,
        allow_ephemeral_fallback: true,
    })?;
    for warning in transport.warnings() {
        tracing::warn!(%warning, "传输层告警");
    }
    Ok(Arc::new(transport))
}

fn cli() -> Command {
    Command::new("sadp-discover-gui")
        .version(sadp_core::VERSION)
        .about("sadp-discover 图形界面（egui）")
        .arg(
            Arg::new("lang")
                .long("lang")
                .value_name("TAG")
                .help("界面语言：en-US 或 zh-CN"),
        )
        .arg(
            Arg::new("port")
                .long("port")
                .value_name("PORT")
                .value_parser(clap::value_parser!(u16))
                .default_value("37020"),
        )
        .arg(
            Arg::new("local-port")
                .long("local-port")
                .value_name("PORT")
                .value_parser(clap::value_parser!(u16)),
        )
        .arg(
            Arg::new("timeout")
                .long("timeout")
                .value_name("MS")
                .value_parser(clap::value_parser!(u64))
                .default_value("1000"),
        )
        .arg(
            Arg::new("repeat")
                .long("repeat")
                .value_name("MS")
                .value_parser(clap::value_parser!(u64))
                .default_value("1000"),
        )
        .arg(
            Arg::new("family")
                .long("family")
                .value_name("v4|v6|both")
                .default_value("both"),
        )
        .arg(
            Arg::new("verbose")
                .short('v')
                .action(ArgAction::Count)
                .help("日志等级：-v info，-vv debug，-vvv trace"),
        )
}

fn scan_config(matches: &clap::ArgMatches) -> ScanConfig {
    let families = match matches.get_one::<String>("family").map(String::as_str) {
        Some("v4") => vec![Family::V4],
        Some("v6") => vec![Family::V6],
        _ => vec![Family::V4, Family::V6],
    };
    ScanConfig {
        port: *matches.get_one::<u16>("port").unwrap_or(&37020),
        timeout: Duration::from_millis(*matches.get_one::<u64>("timeout").unwrap_or(&1000)),
        repeat: Some(Duration::from_millis(
            *matches.get_one::<u64>("repeat").unwrap_or(&1000),
        )),
        families,
        v6_mcast: Ipv6Addr::new(0xff02, 0, 0, 0, 0, 0, 0, 1),
        broadcast: true,
        interfaces: InterfaceSelector::only(Vec::new()),
        ..ScanConfig::default()
    }
}
