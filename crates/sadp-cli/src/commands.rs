//! 非扫描类子命令：`interfaces`、`completions` 与 `logs`。

use std::io::Write;

use sadp_core::i18n::{tr, tr_args};
use sadp_net::udp::{JoinStatus, UdpConfig, UdpTransport};
use sadp_net::{Family, InterfaceInfo, InterfaceSelector, enumerate_interfaces};
use serde::Serialize;

use crate::args::{CompletionsArgs, InterfacesArgs};
use crate::logging;
use crate::output::pad_display;

#[derive(Debug, Serialize)]
struct InterfacesJson<'a> {
    schema: &'static str,
    interfaces: &'a [InterfaceInfo],
    joins: &'a [JoinStatus],
    warnings: &'a [String],
}

/// 打开日志目录；同时打印路径，便于没有桌面环境时直接复制。
pub fn logs() -> u8 {
    let Some(dir) = logging::log_dir() else {
        eprintln!("{}", tr("log.unavailable"));
        return 2;
    };
    if let Err(e) = std::fs::create_dir_all(&dir) {
        eprintln!("{}", tr_args("log.failed", &[("reason", &e.to_string())]));
        return 2;
    }
    println!("{}", dir.display());
    match sadp_core::system::open_dir(&dir) {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("{}", tr_args("log.failed", &[("reason", &e.to_string())]));
            2
        }
    }
}

/// 列出网卡；顺带做一次「绑定 + 组播加入」演练，把失败原因打出来。
pub fn interfaces(args: &InterfacesArgs) -> u8 {
    let list = match enumerate_interfaces(&InterfaceSelector::All) {
        Ok(list) => list,
        Err(e) => {
            eprintln!(
                "{}",
                tr_args("error.no_interface", &[("message", &e.to_string())])
            );
            return 2;
        }
    };

    let (warnings, joins) = match UdpTransport::bind(UdpConfig {
        port: 0,
        families: vec![Family::V4, Family::V6],
        interfaces: InterfaceSelector::All,
        ..UdpConfig::default()
    }) {
        Ok(transport) => (transport.warnings().to_vec(), transport.joins().to_vec()),
        Err(e) => (vec![e.to_string()], Vec::new()),
    };

    if args.json {
        let payload = InterfacesJson {
            schema: "sadp.interfaces/v1",
            interfaces: &list,
            joins: &joins,
            warnings: &warnings,
        };
        match serde_json::to_string_pretty(&payload) {
            Ok(text) => {
                if let Err(e) = writeln!(std::io::stdout(), "{text}") {
                    eprintln!("{}", tr_args("error.io", &[("message", &e.to_string())]));
                    return 2;
                }
            }
            Err(e) => {
                eprintln!("{}", tr_args("error.io", &[("message", &e.to_string())]));
                return 2;
            }
        }
        return 0;
    }

    print_table(&list, &warnings, &joins);
    0
}

fn print_table(list: &[InterfaceInfo], warnings: &[String], joins: &[JoinStatus]) {
    let mut out = std::io::stdout();
    if list.is_empty() {
        println!("{}", tr("interfaces.none"));
        return;
    }

    println!("{}", tr("interfaces.title"));
    let headers = [
        tr("interfaces.name"),
        tr("interfaces.index"),
        tr("interfaces.ipv4"),
        tr("interfaces.ipv6"),
        tr("interfaces.loopback"),
        tr("interfaces.multicast"),
    ];
    let widths = [24usize, 6, 18, 30, 6, 13];

    let header_line: Vec<String> = headers
        .iter()
        .zip(widths)
        .map(|(h, w)| pad_display(h, w))
        .collect();
    let _ = writeln!(out, "{}", header_line.join(" ").trim_end());

    for iface in list {
        let ipv6 = if iface.ipv6.is_empty() {
            "-".to_string()
        } else {
            iface
                .ipv6
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(", ")
        };
        let cells = [
            pad_display(&iface.name, widths[0]),
            pad_display(&iface.index.to_string(), widths[1]),
            pad_display(
                &iface
                    .ipv4
                    .map(|i| i.to_string())
                    .unwrap_or_else(|| "-".into()),
                widths[2],
            ),
            pad_display(&ipv6, widths[3]),
            pad_display(
                &if iface.loopback {
                    tr("common.yes")
                } else {
                    tr("common.no")
                },
                widths[4],
            ),
            pad_display(&join_cell(iface, joins), widths[5]),
        ];
        println!("{}", cells.join(" ").trim_end());
    }

    if !warnings.is_empty() {
        println!();
        for warning in warnings {
            println!("  ! {warning}");
        }
    }
    println!();
    println!("{}", tr("interfaces.note"));
}

/// 每个地址族的组播加入结果，如 `v4✓ v6✓`。
fn join_cell(iface: &InterfaceInfo, joins: &[JoinStatus]) -> String {
    let mut parts = Vec::new();
    for family in [Family::V4, Family::V6] {
        if !iface.has(family) {
            continue;
        }
        let status = joins
            .iter()
            .find(|j| j.iface == iface.name && j.family == family);
        let mark = match status {
            Some(s) if s.ok => "✓",
            Some(_) => "✗",
            None => "-",
        };
        parts.push(format!("{}{mark}", family.as_str()));
    }
    if parts.is_empty() {
        "-".to_string()
    } else {
        parts.join(" ")
    }
}

pub fn completions(args: &CompletionsArgs) {
    let mut command = crate::args::command();
    clap_complete::generate(
        args.shell,
        &mut command,
        "sadp-discover",
        &mut std::io::stdout(),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use unicode_width::UnicodeWidthStr;

    #[test]
    fn table_rows_have_expected_width() {
        let iface = InterfaceInfo {
            name: "以太网 2".into(),
            index: 7,
            ipv4: Some("192.168.1.10".parse().unwrap()),
            netmask: Some("255.255.255.0".parse().unwrap()),
            broadcast: Some("192.168.1.255".parse().unwrap()),
            ipv6: vec!["fe80::1".parse().unwrap()],
            loopback: false,
        };
        // 只验证 pad 后宽度正确，不依赖 stdout 捕获。
        for (value, w) in [(iface.name.clone(), 24usize), (iface.index.to_string(), 6)] {
            assert_eq!(UnicodeWidthStr::width(pad_display(&value, w).as_str()), w);
        }
    }
}
