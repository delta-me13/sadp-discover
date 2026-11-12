//! 端到端：真实 UDP 组播 → 假设备应答 → 引擎解析去重。
//!
//! 需要真实网卡与组播，默认标记 `#[ignore]`；在开发机上用
//! `cargo test -p sadp-fake-device -- --ignored` 手动跑，
//! CI 里只在允许组播的 runner 上启用。

use std::process::{Command, Stdio};
use std::sync::Arc;
use std::time::Duration;

use sadp_core::scan::run_blocking;
use sadp_core::{ScanConfig, SeenVia, Transport};
use sadp_net::udp::{UdpConfig, UdpTransport};

#[test]
#[ignore = "requires a real interface with multicast"]
fn discovers_fake_device_end_to_end() {
    // 假设备活 30s：CI runner 冷启动慢，给足重试窗口。
    let mut fake = Command::new(env!("CARGO_BIN_EXE_sadp-fake-device"))
        .args(["--duration", "30"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn fake device");

    std::thread::sleep(Duration::from_millis(2000));

    let transport = Arc::new(
        UdpTransport::bind(UdpConfig {
            port: 37020,
            // 扫描器绑定临时端口，避免与同机的假设备抢 37020。
            bind_port: Some(0),
            ..UdpConfig::default()
        })
        .expect("bind transport"),
    );

    if transport.interfaces().is_empty() {
        eprintln!("skip: 没有可用网卡，组播环境不满足");
        let _ = fake.kill();
        let _ = fake.wait();
        return;
    }

    // 重试若干轮：组播刚加入、runner 冷启动都可能让第一轮收不到回包。
    let mut session = None;
    for _ in 0..5 {
        let handle: Arc<dyn Transport> = Arc::clone(&transport) as Arc<dyn Transport>;
        let scanned = run_blocking(
            ScanConfig {
                timeout: Duration::from_secs(2),
                ..ScanConfig::default()
            },
            handle,
        )
        .expect("scan");
        let done = !scanned.devices().is_empty();
        session = Some(scanned);
        if done {
            break;
        }
        std::thread::sleep(Duration::from_millis(300));
    }

    let _ = fake.kill();
    let _ = fake.wait();

    let session = session.expect("at least one scan round");
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
