//! 设备集合：跨栈去重、逐字段合并、老化。
//!
//! 合并规则（对应架构文档 §6.3）：
//! - 身份判等优先级 `SN` → `MAC` → `IPv4` → 全局 `IPv6`；
//! - 同一设备可能先以 MAC 被发现、随后补上 SN，因此合并后要**重算去重键**；
//! - 报文字段完整度不同，所以逐字段「非空覆盖」，而不是整条记录替换。

use std::time::{Duration, Instant};

use crate::proto::{Device, DeviceKey};

#[derive(Clone, Debug)]
pub enum Upsert {
    /// 新设备。
    Inserted(Device),
    /// 已有设备字段变化。
    Updated {
        key: DeviceKey,
        changed: Vec<&'static str>,
        device: Device,
    },
    /// 已存在且字段无变化；`refreshed` 表示距上次上报已超过 1 秒，
    /// UI 需要刷新「最近发现」时间（避免每包都重绘）。
    Unchanged {
        key: DeviceKey,
        device: Device,
        refreshed: bool,
    },
    /// 没有任何身份字段的记录，直接丢弃（防御性兜底，正常路径解析层已拦截）。
    Ignored,
}

#[derive(Debug, Default)]
pub struct DeviceStore {
    devices: Vec<Device>,
}

impl DeviceStore {
    #[must_use]
    pub fn new() -> Self {
        Self {
            devices: Vec::new(),
        }
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.devices.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.devices.is_empty()
    }

    #[must_use]
    pub fn devices(&self) -> &[Device] {
        &self.devices
    }

    #[must_use]
    pub fn get(&self, index: usize) -> Option<&Device> {
        self.devices.get(index)
    }

    #[must_use]
    pub fn find(&self, key: &DeviceKey) -> Option<&Device> {
        self.devices.iter().find(|d| &d.key() == key)
    }

    /// 按去重键移除一台设备。
    pub fn remove(&mut self, key: &DeviceKey) -> bool {
        let before = self.devices.len();
        self.devices.retain(|d| &d.key() != key);
        self.devices.len() != before
    }

    /// 插入或合并一条设备记录。
    pub fn upsert(&mut self, incoming: Device) -> Upsert {
        if !incoming.is_identifiable() {
            return Upsert::Ignored;
        }
        if let Some(index) = self.devices.iter().position(|d| d.same_device(&incoming)) {
            let previous_last_seen = self.devices[index].last_seen;
            let changed = self.devices[index].merge_from(&incoming);
            let key = self.devices[index].key();
            if changed.is_empty() {
                let refreshed = self.devices[index]
                    .last_seen
                    .saturating_duration_since(previous_last_seen)
                    >= Duration::from_secs(1);
                return Upsert::Unchanged {
                    key,
                    device: self.devices[index].clone(),
                    refreshed,
                };
            }
            return Upsert::Updated {
                key,
                changed,
                device: self.devices[index].clone(),
            };
        }
        let device = incoming;
        self.devices.push(device.clone());
        Upsert::Inserted(device)
    }

    /// 移除超过 `stale_after` 未再出现的设备，返回被移除的键。
    pub fn prune(&mut self, stale_after: Duration) -> Vec<DeviceKey> {
        let now = Instant::now();
        let mut expired = Vec::new();
        self.devices.retain(|d| {
            let stale = now.saturating_duration_since(d.last_seen) > stale_after;
            if stale {
                expired.push(d.key());
            }
            !stale
        });
        expired
    }

    pub fn clear(&mut self) {
        self.devices.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::proto::{MacAddr, SeenVia};
    use std::net::Ipv4Addr;

    fn device(sn: Option<&str>, mac: Option<&str>, ip: &str) -> Device {
        Device {
            sn: sn.map(str::to_string),
            mac: mac.and_then(MacAddr::parse),
            ipv4: Some(ip.parse::<Ipv4Addr>().unwrap()),
            ..Device::default()
        }
    }

    #[test]
    fn inserts_and_merges_same_device() {
        let mut store = DeviceStore::new();
        let a = device(None, Some("a4:14:37:11:22:33"), "192.168.1.10");
        assert!(matches!(store.upsert(a), Upsert::Inserted(_)));

        let mut b = device(Some("SN-1"), Some("a4:14:37:11:22:33"), "192.168.1.10");
        b.firmware = Some("V1".into());
        match store.upsert(b) {
            Upsert::Updated { key, changed, .. } => {
                assert_eq!(key.0, "sn:SN-1", "合并后应按更强的 SN 重算键");
                assert!(changed.contains(&"sn"));
                assert!(changed.contains(&"firmware"));
            }
            other => panic!("expected update, got {other:?}"),
        }
        assert_eq!(store.len(), 1);
    }

    #[test]
    fn identical_reply_is_unchanged() {
        let mut store = DeviceStore::new();
        let a = device(Some("SN"), Some("a4:14:37:11:22:33"), "192.168.1.10");
        store.upsert(a.clone());
        assert!(matches!(
            store.upsert(a),
            Upsert::Unchanged {
                refreshed: false,
                ..
            }
        ));
    }

    #[test]
    fn distinct_devices_are_kept_apart() {
        let mut store = DeviceStore::new();
        store.upsert(device(Some("SN-A"), None, "192.168.1.10"));
        store.upsert(device(Some("SN-B"), None, "192.168.1.11"));
        assert_eq!(store.len(), 2);
    }

    #[test]
    fn device_without_identity_is_ignored() {
        let mut store = DeviceStore::new();
        let bare = Device {
            seen_via: SeenVia::V4,
            ..Device::default()
        };
        assert!(matches!(store.upsert(bare), Upsert::Ignored));
        store.upsert(device(Some("SN-A"), None, "192.168.1.10"));
        store.upsert(device(Some("SN-B"), None, "192.168.1.11"));
        assert_eq!(store.len(), 2, "幽灵记录不应占一行");
    }

    #[test]
    fn prune_removes_only_stale_entries() {
        let mut store = DeviceStore::new();
        let mut old = device(Some("OLD"), None, "192.168.1.10");
        old.last_seen = Instant::now() - Duration::from_secs(60);
        store.upsert(old);
        store.upsert(device(Some("NEW"), None, "192.168.1.11"));

        let removed = store.prune(Duration::from_secs(30));
        assert_eq!(removed.len(), 1);
        assert_eq!(removed[0].0, "sn:OLD");
        assert_eq!(store.len(), 1);
    }
}
