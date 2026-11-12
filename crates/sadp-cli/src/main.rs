//! `sadp-discover` 入口。
//!
//! 退出码约定（架构文档 §7）：0 发现设备 / 1 未发现 / 2 参数或网络错误 / 3 权限被拒。

mod args;
mod clipboard;
mod commands;
mod logging;
mod output;
mod tui;

use std::io::{IsTerminal, Write};
use std::process::ExitCode;
use std::sync::Arc;

use sadp_core::i18n::{self, tr_args};
use sadp_core::{ScanConfig, Session, Transport};
use sadp_net::udp::{UdpConfig, UdpTransport};
use sadp_net::{InterfaceSelector, NetError};

use crate::args::{Kind, ScanArgs};
use crate::output::Meta;

fn main() -> ExitCode {
    // 语言必须先于 clap 构建确定，否则帮助文本无法本地化。
    let pre = args::pre_scan_lang(std::env::args());
    i18n::init(pre.as_deref());

    let cli = match args::parse() {
        Ok(cli) => cli,
        Err(err) => {
            let _ = err.print();
            return ExitCode::from(u8::try_from(err.exit_code()).unwrap_or(2));
        }
    };

    // 子命令里也可能带 --lang，重新应用一次。
    if let Some(lang) = &cli.lang {
        i18n::init(Some(lang));
    }

    let tui_mode = matches!(&cli.kind, Kind::Scan(scan) if scan.wants_tui())
        && std::io::stdout().is_terminal();
    let _log_guard = logging::init(cli.log_level.as_deref(), cli.verbose, !tui_mode);

    match cli.kind {
        Kind::Completions(comp) => {
            commands::completions(&comp);
            ExitCode::SUCCESS
        }
        Kind::Interfaces(ifaces) => ExitCode::from(commands::interfaces(&ifaces)),
        Kind::Scan(scan) => ExitCode::from(run_scan(scan, tui_mode)),
    }
}

fn run_scan(scan: ScanArgs, tui_mode: bool) -> u8 {
    let scan = normalize_for_mode(scan, tui_mode);
    let notes = scan.notes();
    if !tui_mode {
        for note in &notes {
            eprintln!("{note}");
        }
    }
    let cfg = match scan.scan_config() {
        Ok(cfg) => cfg,
        Err(e) => {
            eprintln!(
                "{}",
                tr_args("error.config", &[("message", &e.to_string())])
            );
            return 2;
        }
    };

    if let Some(dir) = &scan.save_raw
        && let Err(e) = std::fs::create_dir_all(dir)
    {
        eprintln!("{}", tr_args("error.io", &[("message", &e.to_string())]));
        return 2;
    }

    let transport = match UdpTransport::bind(UdpConfig {
        port: scan.port,
        families: scan.families.clone(),
        interfaces: InterfaceSelector::only(scan.interfaces.clone()),
        v6_mcast: scan.v6_mcast,
        broadcast: true,
        bind_v4: scan.bind,
        bind_port: scan.local_port,
        allow_ephemeral_fallback: true,
    }) {
        Ok(transport) => transport,
        Err(e) => {
            eprintln!("{}", tr_args("error.net", &[("message", &e.to_string())]));
            return exit_code_for(&e);
        }
    };

    // 只有「用户没指定本地端口、却没能绑上目标端口」时才提示回退。
    if scan.local_port.is_none() && transport.local_port() != scan.port {
        let note = tr_args(
            "cli.note.fallback_port",
            &[
                ("port", &scan.port.to_string()),
                ("actual", &transport.local_port().to_string()),
            ],
        );
        eprintln!("{note}");
    }
    for warning in transport.warnings() {
        eprintln!("! {warning}");
    }

    let transport: Arc<dyn Transport> = Arc::new(transport);

    if tui_mode {
        return run_tui(cfg, transport, &notes);
    }

    match sadp_core::run_blocking(cfg, transport) {
        Ok(session) => {
            if let Err(message) = emit(&session, &scan) {
                eprintln!("{message}");
                return 2;
            }
            if let Some(key) =
                zero_response_hint(scan.passive, session.devices().len(), session.sadp_packets)
            {
                eprintln!("{}", sadp_core::tr(key));
            }
            if session.devices().is_empty() { 1 } else { 0 }
        }
        Err(e) => {
            eprintln!(
                "{}",
                tr_args("error.config", &[("message", &e.to_string())])
            );
            2
        }
    }
}

fn run_tui(cfg: ScanConfig, transport: Arc<dyn Transport>, notes: &[String]) -> u8 {
    let mut app = match tui::App::new(cfg, transport) {
        Ok(app) => app,
        Err(e) => {
            eprintln!("{}", tr_args("error.net", &[("message", &e.to_string())]));
            return 2;
        }
    };
    // TUI 会切到备用屏幕，stderr 上的提示会被盖住；放进状态栏才看得见。
    if let Some(note) = notes.first() {
        app.set_status(note.clone());
    }
    match app.run() {
        Ok(true) => 0,
        Ok(false) => 1,
        Err(e) => {
            eprintln!("{}", tr_args("error.io", &[("message", &e.to_string())]));
            2
        }
    }
}

/// 非交互模式只输出一轮结果，`--repeat` 没有意义，提示并降级为单轮。
fn normalize_for_mode(mut scan: ScanArgs, tui_mode: bool) -> ScanArgs {
    if !tui_mode && scan.repeat.is_some() {
        eprintln!("{}", sadp_core::tr("cli.note.repeat_needs_tui"));
        scan.repeat = None;
    }
    scan
}

fn emit(session: &Session, scan: &ScanArgs) -> Result<(), String> {
    let meta = Meta::from_session(session, scan.port, &scan.families);

    if let Some(path) = &scan.output {
        let csv = scan.csv
            || path
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("csv"));
        let result = if csv {
            output::export_csv(path, session)
        } else {
            output::export_json(path, session, &meta)
        };
        return result.map_err(|e| {
            tr_args(
                "cli.error.write_failed",
                &[
                    ("path", &path.display().to_string()),
                    ("reason", &e.to_string()),
                ],
            )
        });
    }

    let stdout = std::io::stdout();
    let mut writer = std::io::BufWriter::new(stdout.lock());
    let result = if scan.json {
        output::write_json(&mut writer, session, &meta)
    } else if scan.csv {
        output::write_csv(&mut writer, session)
    } else if scan.quiet {
        output::write_quiet(&mut writer, session)
    } else {
        let width = crossterm::terminal::size()
            .map(|(cols, _)| cols as usize)
            .unwrap_or(120);
        output::write_table(&mut writer, session, &meta, width)
    };
    result
        .and_then(|()| writer.flush())
        .map_err(|e| tr_args("error.io", &[("message", &e.to_string())]))
}

/// 一台设备都没发现、而且连一个 SADP 响应报文都没收到时，给出针对性提示。
///
/// 这和「设备不在线」是两回事：通常意味着本机根本没把包发出去/收回来，
/// 例如 Windows 防火墙、macOS 15+ 的本地网络权限、或网段不对。
fn zero_response_hint(passive: bool, devices: usize, packets: usize) -> Option<&'static str> {
    if passive || devices > 0 || packets > 0 {
        return None;
    }
    Some(if cfg!(target_os = "macos") {
        "error.hint.macos_local_network"
    } else {
        "error.hint.no_sadp_packets"
    })
}

fn exit_code_for(error: &NetError) -> u8 {
    let message = error.to_string().to_ascii_lowercase();
    const PERMISSION_MARKS: [&str; 4] = [
        "os error 10013",
        "os error 13",
        "permission denied",
        "access is denied",
    ];
    if PERMISSION_MARKS.iter().any(|mark| message.contains(mark)) {
        3
    } else {
        2
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zero_response_hint_only_when_nothing_arrived() {
        assert!(zero_response_hint(false, 0, 0).is_some());
        assert!(zero_response_hint(false, 1, 0).is_none(), "有设备就不提示");
        assert!(
            zero_response_hint(false, 0, 3).is_none(),
            "收到过响应就不提示"
        );
        assert!(
            zero_response_hint(true, 0, 0).is_none(),
            "被动模式本来就没响应"
        );
    }

    #[test]
    fn permission_errors_map_to_exit_code_three() {
        let e = NetError::Io("bind 0.0.0.0:37020: Access is denied. (os error 10013)".into());
        assert_eq!(exit_code_for(&e), 3);
        let e = NetError::Io("bind 0.0.0.0:37020: address already in use".into());
        assert_eq!(exit_code_for(&e), 2);
    }

    #[test]
    fn unused_translation_keys_exist_for_notes() {
        // 防止拼错 key 导致运行期显示原始 key。
        for key in [
            "cli.note.fallback_port",
            "cli.error.write_failed",
            "error.config",
            "error.net",
            "error.io",
        ] {
            assert!(sadp_core::i18n::has_key(key), "missing key {key}");
        }
        assert!(!sadp_core::tr("cli.note.fallback_port").is_empty());
    }
}
