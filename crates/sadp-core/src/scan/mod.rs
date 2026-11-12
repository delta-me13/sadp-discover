//! 扫描引擎：配置、事件、设备集合、会话。

pub mod config;
pub mod event;
pub mod session;
pub mod store;

pub use config::{ConfigError, ScanConfig};
pub use event::ScanEvent;
pub use session::{CoreError, Scanner, Session, run_blocking};
pub use store::{DeviceStore, Upsert};

/// 一轮扫描的统计。
#[derive(Clone, Copy, Debug, Default)]
pub struct RoundStats {
    pub id: u64,
    pub found: usize,
    pub elapsed: std::time::Duration,
}
