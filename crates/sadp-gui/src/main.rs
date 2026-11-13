//! sadp-discover 图形界面入口（三阶段 G1）。
//!
//! 只负责参数、日志、传输层绑定与窗口；扫描与渲染逻辑在 app 模块。
//! 事件契约与 TUI 完全相同（ScanEvent），因此两条前端可以并跑对照。

mod app;
mod browser;
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
    let matches = cli().get_matches();
    let verbose = matches.get_count("verbose");
    let _guard = logging::init(None, verbose, true);
    let lang = sadp_core::i18n::init(matches.get_one::<String>("lang").map(String::as_str));
    tracing::info!(?lang, version = sadp_core::VERSION, "GUI 启动");

    let cfg = scan_config(&matches);

    let transport = match UdpTransport::bind(UdpConfig {
        port: cfg.port,
        families: cfg.families.clone(),
        interfaces: cfg.interfaces.clone(),
        v6_mcast: cfg.v6_mcast,
        broadcast: cfg.broadcast,
        bind_v4: None,
        bind_port: matches.get_one::<u16>("local-port").copied(),
        allow_ephemeral_fallback: true,
    }) {
        Ok(transport) => transport,
        Err(err) => {
            eprintln!("{}", tr_args("error.net", &[("message", &err.to_string())]));
            std::process::exit(2);
        }
    };
    for warning in transport.warnings() {
        tracing::warn!(%warning, "传输层告警");
    }
    let transport: Arc<dyn Transport> = Arc::new(transport);

    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_inner_size([1180.0, 740.0])
            .with_min_inner_size([820.0, 520.0])
            .with_title("sadp-discover"),
        ..Default::default()
    };

    eframe::run_native(
        "sadp-discover",
        options,
        Box::new(move |cc| Ok(Box::new(app::App::new(&cc.egui_ctx, cfg, transport)))),
    )
    .map_err(|err| anyhow::anyhow!("窗口初始化失败: {err}"))
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
