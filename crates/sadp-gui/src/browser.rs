//! 用系统默认浏览器打开设备的 Web 后台。
//!
//! 不引入额外依赖，沿用 CLI 剪贴板的做法：调用系统自带工具，保持单文件分发。

use std::io;
use std::process::Command;

use sadp_core::Device;

/// 由设备地址拼出 Web 后台 URL；没有任何地址时返回 `None`。
///
/// 端口取 SADP 上报的 `HTTPPort`，缺省按 80 处理；上报 443 时按 HTTPS 打开
/// （部分固件开启 HTTPS 后会把 `HttpPort` 直接改成 443）。IPv6 地址按 URL 语法加方括号，
/// 链路本地地址补 `%25<scope_id>`（浏览器里 `%` 需要转义成 `%25`）。
pub fn web_url(device: &Device) -> Option<String> {
    let host = if let Some(ip) = device.ipv4 {
        ip.to_string()
    } else {
        let entry = device
            .ipv6
            .iter()
            .find(|e| e.global)
            .or_else(|| device.ipv6.first())?;
        let addr = entry.addr;
        if entry.addr.is_unicast_link_local() && entry.scope_id.unwrap_or(0) != 0 {
            format!("[{addr}%25{}]", entry.scope_id.unwrap_or_default())
        } else {
            format!("[{addr}]")
        }
    };
    let (scheme, port) = match device.http_port.filter(|p| *p != 0) {
        Some(443) => ("https", 443),
        Some(port) => ("http", port),
        None => ("http", 80),
    };
    let default_port = if scheme == "https" { 443 } else { 80 };
    Some(if port == default_port {
        format!("{scheme}://{host}/")
    } else {
        format!("{scheme}://{host}:{port}/")
    })
}

/// 交给系统默认浏览器打开。只负责拉起进程，不等待浏览器退出。
pub fn open(url: &str) -> io::Result<()> {
    let mut child = opener(url).spawn()?;
    // 不阻塞 UI；子进程退出后由这个线程回收，避免 Unix 上留下僵尸进程。
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}

#[cfg(target_os = "windows")]
fn opener(url: &str) -> Command {
    // 直接调 FileProtocolHandler，不经过 cmd，避免 URL 里的字符被 shell 解释。
    let mut cmd = Command::new("rundll32");
    cmd.arg("url.dll,FileProtocolHandler").arg(url);
    cmd
}

#[cfg(target_os = "macos")]
fn opener(url: &str) -> Command {
    let mut cmd = Command::new("open");
    cmd.arg(url);
    cmd
}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
fn opener(url: &str) -> Command {
    let mut cmd = Command::new("xdg-open");
    cmd.arg(url);
    cmd
}

#[cfg(test)]
mod tests {
    use super::*;
    use sadp_core::Ipv6Endpoint;
    use std::net::{Ipv4Addr, Ipv6Addr};

    #[test]
    fn ipv4_defaults_to_port_80() {
        let device = Device {
            ipv4: Some(Ipv4Addr::new(192, 168, 1, 64)),
            ..Device::default()
        };
        assert_eq!(web_url(&device).as_deref(), Some("http://192.168.1.64/"));
    }

    #[test]
    fn ipv4_keeps_non_default_http_port() {
        let device = Device {
            ipv4: Some(Ipv4Addr::new(192, 168, 1, 64)),
            http_port: Some(8080),
            ..Device::default()
        };
        assert_eq!(
            web_url(&device).as_deref(),
            Some("http://192.168.1.64:8080/")
        );
    }

    #[test]
    fn port_443_switches_to_https() {
        let device = Device {
            ipv4: Some(Ipv4Addr::new(192, 168, 1, 64)),
            http_port: Some(443),
            ..Device::default()
        };
        assert_eq!(web_url(&device).as_deref(), Some("https://192.168.1.64/"));
    }

    #[test]
    fn ipv6_global_is_bracketed() {
        let device = Device {
            ipv6: vec![Ipv6Endpoint {
                addr: "2001:db8::64".parse::<Ipv6Addr>().unwrap(),
                scope_id: None,
                global: true,
            }],
            ..Device::default()
        };
        assert_eq!(web_url(&device).as_deref(), Some("http://[2001:db8::64]/"));
    }

    #[test]
    fn link_local_ipv6_carries_escaped_scope() {
        let device = Device {
            ipv6: vec![Ipv6Endpoint {
                addr: "fe80::1".parse::<Ipv6Addr>().unwrap(),
                scope_id: Some(11),
                global: false,
            }],
            ..Device::default()
        };
        assert_eq!(web_url(&device).as_deref(), Some("http://[fe80::1%2511]/"));
    }

    #[test]
    fn no_address_means_no_url() {
        assert!(web_url(&Device::default()).is_none());
    }
}
