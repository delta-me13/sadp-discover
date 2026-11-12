//! 扫描事件：引擎与所有 UI/导出层之间唯一的契约。
//!
//! 事件只描述「发生了什么」，不包含任何展示文案，便于 TUI、GUI、JSON 共用。

use std::net::SocketAddr;
use std::time::{Duration, Instant};

use sadp_net::NetError;

use crate::proto::{Device, DeviceKey};

#[derive(Clone, Debug)]
pub enum ScanEvent {
    /// 新一轮开始。`id` 从 1 递增。
    RoundStarted { id: u64, at: Instant },
    /// 首次发现某台设备。
    DeviceFound(Device),
    /// 已知设备字段发生变化。
    DeviceUpdated {
        key: DeviceKey,
        /// 发生变化的字段名（英文静态串，不做本地化）。
        changed: Vec<&'static str>,
        /// 合并后的完整设备快照。
        device: Device,
    },
    /// 收到无法解析的报文（伪造、其他协议、损坏包）。
    DecodeFailed { src: SocketAddr, error: String },
    /// 网络层错误。单族不可用时只出现一次，不终止扫描。
    NetError(NetError),
    /// 一轮结束。`found` 是当前累计设备数。
    RoundFinished {
        id: u64,
        found: usize,
        elapsed: Duration,
        /// 累计收到的「看起来像 ProbeMatch」的数据报数量。
        /// 为 0 通常意味着本机根本没收到响应（防火墙/权限/网段不对）。
        sadp_packets: usize,
    },
    /// 设备因长时间未响应被移除。
    DeviceExpired { key: DeviceKey },
    /// 扫描线程退出（单轮模式结束或收到停止信号）。
    Stopped,
}

impl ScanEvent {
    /// 是否属于「设备集合发生变化」的事件。
    #[must_use]
    pub const fn is_device_event(&self) -> bool {
        matches!(
            self,
            Self::DeviceFound(_) | Self::DeviceUpdated { .. } | Self::DeviceExpired { .. }
        )
    }
}
