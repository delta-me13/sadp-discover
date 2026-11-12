//! 引擎端到端：用内存传输层验证双栈去重、合并、被动模式与错误处理。

use std::net::{SocketAddr, SocketAddrV6};
use std::sync::Arc;
use std::time::Duration;

use sadp_core::scan::run_blocking;
use sadp_core::{Family, ScanConfig, SeenVia};
use sadp_net::MockTransport;

const V4_IPC: &[u8] = br#"<?xml version="1.0" encoding="utf-8"?>
<ProbeMatch>
  <Types>inquiry</Types>
  <DeviceType>DS-2CD2T47G2-L</DeviceType>
  <DeviceSN>DS-2CD2T47G2-L20210618AAWRL12345678</DeviceSN>
  <MAC>a4:14:37:5f:4c:6d</MAC>
  <IPv4Address>192.168.1.64</IPv4Address>
  <IPv4SubnetMask>255.255.255.0</IPv4SubnetMask>
  <IPv4Gateway>192.168.1.1</IPv4Gateway>
  <DHCP>false</DHCP>
  <DSPVersion>V5.7.3 build 210618</DSPVersion>
  <Activated>true</Activated>
</ProbeMatch>"#;

/// 同一台设备的 IPv6 应答：只有 MAC 与 IPv6 字段，用来验证跨栈合并。
const V6_SAME_DEVICE: &[u8] = br#"<ProbeMatch>
  <Types>inquiry</Types>
  <MAC>a4:14:37:5f:4c:6d</MAC>
  <IPv6Address>fe80::21b:2bff:fe3c:4d5e</IPv6Address>
  <IPv6Address>2001:db8::64</IPv6Address>
  <IPv6Masklen>64</IPv6Masklen>
</ProbeMatch>"#;

const V4_OTHER: &[u8] = br#"<ProbeMatch>
  <Types>inquiry</Types>
  <DeviceSN>DS-7608N-K2-OTHER</DeviceSN>
  <MAC>44:19:b6:aa:bb:cc</MAC>
  <IPv4Address>192.168.1.65</IPv4Address>
</ProbeMatch>"#;

fn cfg(timeout_ms: u64) -> ScanConfig {
    ScanConfig {
        timeout: Duration::from_millis(timeout_ms),
        ..ScanConfig::default()
    }
}

fn v6_src(addr: &str) -> SocketAddr {
    SocketAddr::V6(SocketAddrV6::new(addr.parse().unwrap(), 37020, 0, 1))
}

#[test]
fn same_device_seen_over_v4_and_v6_is_merged() {
    let transport = Arc::new(MockTransport::single());
    transport.push_bytes(Family::V4, "192.168.1.64:37020".parse().unwrap(), V4_IPC);
    transport.push_bytes(Family::V6, v6_src("fe80::1"), V6_SAME_DEVICE);

    let session = run_blocking(cfg(200), transport).expect("scan");

    assert_eq!(session.devices().len(), 1, "双栈必须合并成一行");
    let d = &session.devices()[0];
    assert_eq!(d.seen_via, SeenVia::Both);
    assert_eq!(d.sn.as_deref(), Some("DS-2CD2T47G2-L20210618AAWRL12345678"));
    assert_eq!(d.ipv4.unwrap().to_string(), "192.168.1.64");
    assert_eq!(d.ipv6.len(), 2);
    assert!(d.seen_on.contains(&"mock0".to_string()));
    assert_eq!(session.decode_failures, 0);
    assert_eq!(session.sadp_packets, 2, "两个 ProbeMatch 都应计数");
    assert!(session.finished);
}

#[test]
fn distinct_devices_are_listed_separately() {
    let transport = Arc::new(MockTransport::single());
    transport.push_bytes(Family::V4, "192.168.1.64:37020".parse().unwrap(), V4_IPC);
    transport.push_bytes(Family::V4, "192.168.1.65:37020".parse().unwrap(), V4_OTHER);

    let session = run_blocking(cfg(150), transport).expect("scan");
    assert_eq!(session.devices().len(), 2);
}

#[test]
fn garbage_datagram_is_reported_not_fatal() {
    let transport = Arc::new(MockTransport::single());
    transport.push_bytes(Family::V4, "192.168.1.99:37020".parse().unwrap(), b"hello");
    transport.push_bytes(Family::V4, "192.168.1.64:37020".parse().unwrap(), V4_IPC);

    let session = run_blocking(cfg(150), transport).expect("scan");
    assert_eq!(session.devices().len(), 1, "坏包不能影响好包");
    assert_eq!(session.decode_failures, 1);
    assert_eq!(session.sadp_packets, 1, "只有 ProbeMatch 计入响应包");
}

#[test]
fn unknown_only_payload_does_not_create_ghost_device() {
    let transport = Arc::new(MockTransport::single());
    transport.push_bytes(
        Family::V4,
        "192.168.1.200:37020".parse().unwrap(),
        br#"<ProbeMatch><Types>inquiry</Types><VendorTag>7</VendorTag></ProbeMatch>"#,
    );
    let session = run_blocking(cfg(120), transport).expect("scan");
    assert!(
        session.devices().is_empty(),
        "没有身份字段的报文不能变成设备"
    );
    assert_eq!(session.decode_failures, 1);
    assert_eq!(session.sadp_packets, 1);
}

#[test]
fn junk_only_means_zero_sadp_packets() {
    let transport = Arc::new(MockTransport::single());
    transport.push_bytes(Family::V4, "192.168.1.99:37020".parse().unwrap(), b"hello");
    let session = run_blocking(cfg(120), transport).expect("scan");
    assert_eq!(session.sadp_packets, 0, "杂散报文不是 SADP 响应");
    assert!(session.devices().is_empty());
}

#[test]
fn passive_mode_sends_no_probe() {
    let transport = Arc::new(MockTransport::single());
    transport.push_bytes(Family::V4, "192.168.1.64:37020".parse().unwrap(), V4_IPC);
    let config = ScanConfig {
        passive: true,
        ..cfg(150)
    };
    let as_dyn: Arc<dyn sadp_core::Transport> = transport.clone();
    let session = run_blocking(config, as_dyn).expect("scan");
    assert!(transport.sent_probes().is_empty());
    assert_eq!(session.devices().len(), 1);
}

#[test]
fn repeat_mode_runs_multiple_rounds_until_limit() {
    let transport = Arc::new(MockTransport::single());
    transport.push_bytes(Family::V4, "192.168.1.64:37020".parse().unwrap(), V4_IPC);
    let config = ScanConfig {
        repeat: Some(Duration::from_millis(150)),
        ..cfg(100)
    };

    // 连续模式不会自己停，用超时轮询三轮后手动停。
    let mut scanner = sadp_core::Scanner::start(config, transport).expect("start");
    let mut rounds = 0;
    let mut session = sadp_core::Session::new();
    let deadline = std::time::Instant::now() + Duration::from_secs(3);
    while std::time::Instant::now() < deadline {
        for ev in scanner.poll(Duration::from_millis(50)) {
            session.apply(&ev);
            if matches!(ev, sadp_core::ScanEvent::RoundFinished { .. }) {
                rounds += 1;
            }
        }
        if rounds >= 3 {
            break;
        }
    }
    scanner.stop();
    assert!(rounds >= 2, "连续模式应至少跑两轮，实际 {rounds}");
    assert_eq!(session.devices().len(), 1);
}
