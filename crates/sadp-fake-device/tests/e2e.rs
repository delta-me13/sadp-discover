//! 端到端：真实 UDP 组播 → 假设备应答 → 引擎解析去重。
//!
//! 需要真实网卡与组播，默认标记 `#[ignore]`；在开发机上用
//! `cargo test -p sadp-fake-device -- --ignored` 手动跑，
//! CI 里只在允许组播的 runner 上启用。

use std::process::{Command, Stdio};
use std::sync::Arc;
use std::time::Duration;

use sadp_core::scan::run_blocking;
use sadp_core::{ScanConfig, SeenVia};
use sadp_net::udp::{UdpConfig, UdpTransport};

#[test]
#[ignore = "requires a real interface with multicast"]
fn discovers_fake_device_end_to_end() {
    let mut fake = Command::new(env!("CARGO_BIN_EXE_sadp-fake-device"))
        .args(["--duration", "10"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn fake device");

    std::thread::sleep(Duration::from_millis(1500));

    let transport = UdpTransport::bind(UdpConfig {
        port: 37020,
        // 扫描器绑定临时端口，避免与同机的假设备抢 37020。
        bind_port: Some(0),
        ..UdpConfig::default()
    })
    .expect("bind transport");

    let session = run_blocking(
        ScanConfig {
            timeout: Duration::from_secs(2),
            ..ScanConfig::default()
        },
        Arc::new(transport),
    )
    .expect("scan");

    let _ = fake.kill();
    let _ = fake.wait();

    assert!(
        !session.devices().is_empty(),
        "should discover the fake device, errors={:?}",
        session.errors
    );
    let device = &session.devices()[0];
    assert!(device.sn.is_some());
    assert!(device.ipv4.is_some());
    assert!(
        matches!(device.seen_via, SeenVia::V4 | SeenVia::Both),
        "via={:?}",
        device.seen_via
    );
}
