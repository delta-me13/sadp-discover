//! 内存传输层，用于无网卡的单测与确定性回放。
//!
//! 引擎的全部行为（跨栈去重、字段合并、超时、乱序）都可以在这里脚本化验证，
//! 不需要真网卡，也不需要管理员权限。

use std::collections::VecDeque;
use std::net::SocketAddr;
use std::sync::Mutex;
use std::time::Duration;

use crate::{Datagram, Family, InterfaceInfo, NetError, Transport};

pub struct MockTransport {
    interfaces: Vec<InterfaceInfo>,
    families: Vec<Family>,
    queue: Mutex<VecDeque<Datagram>>,
    sent: Mutex<Vec<Vec<u8>>>,
    sent_families: Mutex<Vec<Family>>,
    port: u16,
}

impl MockTransport {
    #[must_use]
    pub fn new(interfaces: Vec<InterfaceInfo>, families: Vec<Family>) -> Self {
        Self {
            interfaces,
            families,
            queue: Mutex::new(VecDeque::new()),
            sent: Mutex::new(Vec::new()),
            sent_families: Mutex::new(Vec::new()),
            port: crate::SADP_PORT,
        }
    }

    /// 单网卡、双栈的常用构造。
    #[must_use]
    pub fn single() -> Self {
        Self::new(
            vec![InterfaceInfo {
                name: "mock0".into(),
                index: 1,
                ipv4: Some("192.168.1.10".parse().unwrap()),
                netmask: Some("255.255.255.0".parse().unwrap()),
                broadcast: Some("192.168.1.255".parse().unwrap()),
                ipv6: vec!["fe80::1".parse().unwrap()],
                loopback: false,
            }],
            vec![Family::V4, Family::V6],
        )
    }

    /// 排入一条原始报文。
    pub fn push_bytes(&self, family: Family, src: SocketAddr, bytes: impl Into<Vec<u8>>) {
        let iface = self
            .interfaces
            .iter()
            .find(|i| i.has(family))
            .cloned()
            .unwrap_or_else(|| self.interfaces[0].clone());
        let dg = Datagram {
            bytes: bytes.into(),
            src,
            iface,
            family,
            local: SocketAddr::from(([0, 0, 0, 0], self.port)),
        };
        self.queue.lock().expect("mock queue").push_back(dg);
    }

    #[must_use]
    pub fn sent_probes(&self) -> Vec<Vec<u8>> {
        self.sent.lock().expect("mock sent").clone()
    }

    #[must_use]
    pub fn sent_families(&self) -> Vec<Family> {
        self.sent_families.lock().expect("mock families").clone()
    }

    #[must_use]
    pub fn pending(&self) -> usize {
        self.queue.lock().expect("mock queue").len()
    }
}

impl Transport for MockTransport {
    fn interfaces(&self) -> &[InterfaceInfo] {
        &self.interfaces
    }

    fn send_probe(&self, families: &[Family], payload: &[u8]) -> Result<(), NetError> {
        self.sent.lock().expect("mock sent").push(payload.to_vec());
        self.sent_families
            .lock()
            .expect("mock families")
            .extend_from_slice(families);
        Ok(())
    }

    fn recv(&self, timeout: Duration) -> Result<Option<Datagram>, NetError> {
        if let Some(dg) = self.queue.lock().expect("mock queue").pop_front() {
            return Ok(Some(dg));
        }
        // 模拟阻塞，让引擎的轮次计时与真实传输层行为一致；
        // 但不要真的睡满 3 秒，测试需要快。
        std::thread::sleep(timeout.min(Duration::from_millis(2)));
        Ok(None)
    }

    fn local_port(&self) -> u16 {
        self.port
    }

    fn bound_families(&self) -> &[Family] {
        &self.families
    }
}
