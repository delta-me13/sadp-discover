//! 扫描配置。

use std::net::Ipv6Addr;
use std::time::Duration;

use sadp_net::iface::InterfaceSelector;
use sadp_net::{Family, MCAST_V6_DEFAULT, SADP_PORT};

/// 一轮扫描的参数。
#[derive(Clone, Debug)]
pub struct ScanConfig {
    pub port: u16,
    /// 单轮收包窗口。
    pub timeout: Duration,
    /// `None` = 只扫一轮；`Some` = 按周期连续扫描。
    pub repeat: Option<Duration>,
    pub families: Vec<Family>,
    /// IPv6 组播组（预研前可配置，见架构文档 §2.4）。
    pub v6_mcast: Ipv6Addr,
    pub broadcast: bool,
    pub interfaces: InterfaceSelector,
    /// 被动模式：不发 Probe，只收设备主动通告。
    pub passive: bool,
    /// 超过该时长未再出现的设备从结果中移除；`None` = 不老化。
    pub stale_after: Option<Duration>,
    /// 把原始报文落盘到该目录（排障用，默认关闭）。
    pub save_raw: Option<std::path::PathBuf>,
}

impl Default for ScanConfig {
    fn default() -> Self {
        Self {
            port: SADP_PORT,
            timeout: Duration::from_secs(3),
            repeat: None,
            families: vec![Family::V4, Family::V6],
            v6_mcast: MCAST_V6_DEFAULT,
            broadcast: true,
            interfaces: InterfaceSelector::Auto,
            passive: false,
            stale_after: None,
            save_raw: None,
        }
    }
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ConfigError {
    #[error("timeout must be greater than zero")]
    ZeroTimeout,
    #[error("port must not be zero")]
    ZeroPort,
    #[error("repeat interval must be at least the scan window")]
    RepeatShorterThanTimeout,
    #[error("at least one address family must be enabled")]
    NoFamily,
}

impl ScanConfig {
    pub fn validate(&self) -> Result<(), ConfigError> {
        if self.timeout.is_zero() {
            return Err(ConfigError::ZeroTimeout);
        }
        if self.port == 0 {
            return Err(ConfigError::ZeroPort);
        }
        if self.families.is_empty() {
            return Err(ConfigError::NoFamily);
        }
        if let Some(repeat) = self.repeat
            && repeat < self.timeout
        {
            return Err(ConfigError::RepeatShorterThanTimeout);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_is_valid() {
        assert!(ScanConfig::default().validate().is_ok());
    }

    #[test]
    fn rejects_nonsense() {
        let no_timeout = ScanConfig {
            timeout: Duration::ZERO,
            ..ScanConfig::default()
        };
        assert_eq!(no_timeout.validate(), Err(ConfigError::ZeroTimeout));

        let no_port = ScanConfig {
            port: 0,
            ..ScanConfig::default()
        };
        assert_eq!(no_port.validate(), Err(ConfigError::ZeroPort));

        let no_family = ScanConfig {
            families: Vec::new(),
            ..ScanConfig::default()
        };
        assert_eq!(no_family.validate(), Err(ConfigError::NoFamily));

        let short_repeat = ScanConfig {
            timeout: Duration::from_secs(5),
            repeat: Some(Duration::from_secs(2)),
            ..ScanConfig::default()
        };
        assert_eq!(
            short_repeat.validate(),
            Err(ConfigError::RepeatShorterThanTimeout)
        );
    }
}
