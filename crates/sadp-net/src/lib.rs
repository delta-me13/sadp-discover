//! SADP 平台传输层。
//!
//! **socket / 网卡相关的 `cfg(target_os)` 只出现在这里。**
//! 上层（`sadp-core` / `sadp-cli` / `sadp-gui`）只看到 `Transport` trait、`Family`、`Datagram`；
//! 调用系统工具（剪贴板、默认浏览器）的平台分支由各自前端按需处理。
//!
//! 职责：
//! - 枚举网卡（含 v4 掩码/广播地址、v6 地址、ifindex）；
//! - 每个地址族一个接收 socket，逐网卡设置组播出口并加入组播组；
//! - 发送 Probe 到组播组与各网卡子网广播地址；
//! - 把收到的数据报连同「来自哪块网卡、哪个地址族」交给上层。

pub mod iface;
pub mod mock;
pub mod udp;

use std::fmt;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::time::Duration;

use serde::Serialize;

pub use iface::{InterfaceSelector, enumerate_interfaces};
pub use mock::MockTransport;
pub use udp::{UdpConfig, UdpTransport};

/// SADP 固定端口。官方工具与设备都使用 37020。
pub const SADP_PORT: u16 = 37020;
/// IPv4 组播组：与 ONVIF/WS-Discovery 复用同一地址，但协议完全不同。
pub const MCAST_V4: Ipv4Addr = Ipv4Addr::new(239, 255, 255, 250);
/// IPv6 候选组播组（**未经抓包证实**，见架构文档 §2.4，可用 `--v6-mcast` 覆盖）。
pub const MCAST_V6_DEFAULT: Ipv6Addr = Ipv6Addr::new(0xff02, 0, 0, 0, 0, 0, 0, 1);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
pub enum Family {
    #[serde(rename = "v4")]
    V4,
    #[serde(rename = "v6")]
    V6,
}

impl Family {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::V4 => "v4",
            Self::V6 => "v6",
        }
    }

    #[must_use]
    pub const fn display_name(self) -> &'static str {
        match self {
            Self::V4 => "IPv4",
            Self::V6 => "IPv6",
        }
    }
}

impl fmt::Display for Family {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl std::str::FromStr for Family {
    type Err = NetError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_ascii_lowercase().as_str() {
            "v4" | "4" | "ipv4" => Ok(Self::V4),
            "v6" | "6" | "ipv6" => Ok(Self::V6),
            other => Err(NetError::BadFamily(other.to_string())),
        }
    }
}

/// 一块网卡（按适配器聚合 v4/v6 地址）。
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct InterfaceInfo {
    pub name: String,
    /// 组播出口与 v6 链路本地作用域都需要。
    pub index: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ipv4: Option<Ipv4Addr>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub netmask: Option<Ipv4Addr>,
    /// 子网定向广播地址，优先于 `255.255.255.255`。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub broadcast: Option<Ipv4Addr>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub ipv6: Vec<Ipv6Addr>,
    pub loopback: bool,
}

impl InterfaceInfo {
    #[must_use]
    pub fn has(&self, family: Family) -> bool {
        match family {
            Family::V4 => self.ipv4.is_some(),
            Family::V6 => !self.ipv6.is_empty(),
        }
    }

    /// 判断某个源地址是否落在该网卡的 v4 子网内（用于把回包归属到网卡）。
    #[must_use]
    pub fn contains_v4(&self, ip: Ipv4Addr) -> bool {
        match (self.ipv4, self.netmask) {
            (Some(net), Some(mask)) => {
                let m = u32::from(mask);
                (u32::from(net) & m) == (u32::from(ip) & m)
            }
            _ => false,
        }
    }

    /// 展示用的简短标签：`以太网 (192.168.1.10)`。
    #[must_use]
    pub fn label(&self) -> String {
        let addr = self
            .ipv4
            .map(|a| a.to_string())
            .or_else(|| self.ipv6.first().map(ToString::to_string))
            .unwrap_or_else(|| "-".to_string());
        format!("{} ({addr})", self.name)
    }
}

/// 一条收到的数据报。
#[derive(Clone, Debug)]
pub struct Datagram {
    pub bytes: Vec<u8>,
    /// 源地址；v6 链路本地地址的 `scope_id` 已填写。
    pub src: SocketAddr,
    /// 归属网卡。按源地址所在子网 / v6 scope 判定。
    pub iface: InterfaceInfo,
    pub family: Family,
    /// 本机接收地址，便于排障。
    pub local: SocketAddr,
}

impl Datagram {
    #[must_use]
    pub fn src_ip(&self) -> IpAddr {
        self.src.ip()
    }
}

#[derive(Debug, thiserror::Error, Clone, PartialEq, Eq)]
pub enum NetError {
    #[error("socket error: {0}")]
    Io(String),
    #[error("no usable interface for {0}")]
    NoInterface(String),
    #[error("unsupported address family: {0}")]
    BadFamily(String),
    #[error("transport closed")]
    Closed,
}

impl From<std::io::Error> for NetError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e.to_string())
    }
}

impl NetError {
    /// 是否为「该地址族在本机不可用」（例如 IPv4 协议栈被禁用）。
    /// 这类错误只降级该族，不应让程序退出。
    #[must_use]
    pub fn is_family_unavailable(&self) -> bool {
        const MARKS: [&str; 6] = [
            "address family not supported",
            "protocol not supported",
            "10047", // WSAEAFNOSUPPORT
            "97",    // EAFNOSUPPORT (linux)
            "10043", // WSAEPROTONOSUPPORT
            "not supported",
        ];
        match self {
            Self::Io(msg) => {
                let m = msg.to_ascii_lowercase();
                MARKS.iter().any(|mark| m.contains(mark))
            }
            Self::NoInterface(_) => true,
            _ => false,
        }
    }
}

/// 传输层抽象。单测注入 `MockTransport`，生产用 `UdpTransport`。
pub trait Transport: Send + Sync {
    /// 本次扫描可用的网卡快照。
    fn interfaces(&self) -> &[InterfaceInfo];

    /// 向所有可用网卡发送一次 Probe。
    fn send_probe(&self, families: &[Family], payload: &[u8]) -> Result<(), NetError>;

    /// 阻塞至多 `timeout`，取一条数据报。
    fn recv(&self, timeout: Duration) -> Result<Option<Datagram>, NetError>;

    /// 实际绑定的本地端口（绑定 37020 失败时会退到临时端口）。
    fn local_port(&self) -> u16;

    /// 实际成功绑定的地址族。
    fn bound_families(&self) -> &[Family];

    /// 累计成功发出的数据报数量（按网卡计）。
    ///
    /// 用来区分「包根本没发出去」（权限 / 网卡不可用）与「发出去了但没人回」
    /// （设备离线 / 网段不对 / 防火墙丢包）——两者的提示语不该一样。
    /// 默认 0：测试替身可以不实现。
    fn sent_packets(&self) -> usize {
        0
    }
}
