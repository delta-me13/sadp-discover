//! 视图逻辑：排序与过滤。
//!
//! 纯函数，TUI 与三阶段的 GUI 共用同一套规则（架构文档 G0）。
//! 这里不产生任何文案，列标题等由调用方通过 i18n key 取。

use std::cmp::Ordering;

use crate::proto::Device;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum SortKey {
    #[default]
    Ip,
    Mac,
    Model,
    Sn,
    Via,
    Activated,
    Firmware,
    Description,
}

impl SortKey {
    pub const ALL: [Self; 8] = [
        Self::Ip,
        Self::Mac,
        Self::Model,
        Self::Sn,
        Self::Via,
        Self::Activated,
        Self::Firmware,
        Self::Description,
    ];

    #[must_use]
    pub const fn next(self) -> Self {
        match self {
            Self::Ip => Self::Mac,
            Self::Mac => Self::Model,
            Self::Model => Self::Sn,
            Self::Sn => Self::Via,
            Self::Via => Self::Activated,
            Self::Activated => Self::Firmware,
            Self::Firmware => Self::Description,
            Self::Description => Self::Ip,
        }
    }

    /// 对应的 i18n key（列标题复用同一套）。
    #[must_use]
    pub const fn label_key(self) -> &'static str {
        match self {
            Self::Ip => "tui.col.ip",
            Self::Mac => "tui.col.mac",
            Self::Model => "tui.col.model",
            Self::Sn => "tui.col.sn",
            Self::Via => "tui.col.via",
            Self::Activated => "tui.col.activated",
            Self::Firmware => "tui.detail.firmware",
            Self::Description => "tui.col.description",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum SortDir {
    #[default]
    Asc,
    Desc,
}

impl SortDir {
    #[must_use]
    pub const fn flip(self) -> Self {
        match self {
            Self::Asc => Self::Desc,
            Self::Desc => Self::Asc,
        }
    }

    #[must_use]
    pub const fn arrow(self) -> &'static str {
        match self {
            Self::Asc => "▲",
            Self::Desc => "▼",
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct ViewOptions {
    pub filter: String,
    pub sort: SortKey,
    pub dir: SortDir,
}

/// 大小写不敏感地匹配 SN / MAC / 地址 / 型号 / 描述 / 固件 / 来源网卡。
#[must_use]
pub fn matches_filter(device: &Device, needle: &str) -> bool {
    let needle = needle.trim().to_lowercase();
    if needle.is_empty() {
        return true;
    }
    let mut haystacks: Vec<String> = Vec::with_capacity(12);
    if let Some(sn) = &device.sn {
        haystacks.push(sn.clone());
    }
    if let Some(mac) = device.mac {
        haystacks.push(mac.to_string());
        haystacks.push(mac.to_compact());
    }
    if let Some(ip) = device.ipv4 {
        haystacks.push(ip.to_string());
    }
    for ep in &device.ipv6 {
        haystacks.push(ep.addr.to_string());
    }
    if let Some(model) = &device.model {
        haystacks.push(model.clone());
    }
    if let Some(desc) = &device.description {
        haystacks.push(desc.clone());
    }
    if let Some(fw) = &device.firmware {
        haystacks.push(fw.clone());
    }
    for iface in &device.seen_on {
        haystacks.push(iface.clone());
    }
    haystacks.push(device.seen_via.as_str().to_string());

    haystacks.iter().any(|h| h.to_lowercase().contains(&needle))
}

/// 返回按当前排序/过滤规则可见的设备下标。
#[must_use]
pub fn visible_indices(devices: &[Device], opts: &ViewOptions) -> Vec<usize> {
    let mut indices: Vec<usize> = devices
        .iter()
        .enumerate()
        .filter(|(_, d)| matches_filter(d, &opts.filter))
        .map(|(i, _)| i)
        .collect();

    indices.sort_by(|&a, &b| {
        compare(&devices[a], &devices[b], opts.sort, opts.dir)
            .then_with(|| devices[a].key().cmp(&devices[b].key()))
    });
    indices
}

fn compare(a: &Device, b: &Device, key: SortKey, dir: SortDir) -> Ordering {
    match key {
        SortKey::Ip => cmp_opt(ip_key(a).as_ref(), ip_key(b).as_ref(), dir),
        SortKey::Mac => cmp_opt(a.mac.as_ref(), b.mac.as_ref(), dir),
        SortKey::Model => cmp_opt(a.display_model(), b.display_model(), dir),
        SortKey::Sn => cmp_opt(a.sn.as_deref(), b.sn.as_deref(), dir),
        SortKey::Via => cmp_opt(
            Some(&via_rank(a.seen_via)),
            Some(&via_rank(b.seen_via)),
            dir,
        ),
        SortKey::Activated => cmp_opt(a.activated.as_ref(), b.activated.as_ref(), dir),
        SortKey::Firmware => cmp_opt(a.firmware.as_ref(), b.firmware.as_ref(), dir),
        SortKey::Description => cmp_opt(a.description.as_ref(), b.description.as_ref(), dir),
    }
}

/// 缺失值永远排在最后（升序降序都一样），避免空值把有信息的行挤走。
fn cmp_opt<T: Ord + ?Sized>(a: Option<&T>, b: Option<&T>, dir: SortDir) -> Ordering {
    match (a, b) {
        (Some(a), Some(b)) => match dir {
            SortDir::Asc => a.cmp(b),
            SortDir::Desc => b.cmp(a),
        },
        (None, None) => Ordering::Equal,
        (None, Some(_)) => Ordering::Greater,
        (Some(_), None) => Ordering::Less,
    }
}

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
enum IpKey {
    V4(u32),
    V6(u128),
}

fn ip_key(d: &Device) -> Option<IpKey> {
    if let Some(ip) = d.ipv4 {
        return Some(IpKey::V4(u32::from(ip)));
    }
    let best = d
        .ipv6
        .iter()
        .find(|e| e.global)
        .or_else(|| d.ipv6.first())?;
    Some(IpKey::V6(u128::from(best.addr)))
}

fn via_rank(via: crate::proto::SeenVia) -> u8 {
    match via {
        crate::proto::SeenVia::V4 => 0,
        crate::proto::SeenVia::V6 => 1,
        crate::proto::SeenVia::Both => 2,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::proto::{Ipv6Endpoint, MacAddr, SeenVia};

    fn dev(ip: Option<&str>, mac: Option<&str>, model: Option<&str>) -> Device {
        Device {
            ipv4: ip.map(|s| s.parse().unwrap()),
            mac: mac.and_then(MacAddr::parse),
            model: model.map(str::to_string),
            ..Device::default()
        }
    }

    #[test]
    fn ip_sorting_is_numeric_not_lexicographic() {
        let devices = vec![
            dev(Some("192.168.1.100"), None, None),
            dev(Some("192.168.1.9"), None, None),
            dev(Some("192.168.1.64"), None, None),
        ];
        let opts = ViewOptions::default();
        let idx = visible_indices(&devices, &opts);
        let ips: Vec<_> = idx
            .iter()
            .map(|&i| devices[i].ipv4.unwrap().to_string())
            .collect();
        assert_eq!(ips, ["192.168.1.9", "192.168.1.64", "192.168.1.100"]);
    }

    #[test]
    fn missing_values_sort_last_in_both_directions() {
        let devices = vec![
            dev(None, None, Some("B")),
            dev(Some("10.0.0.1"), None, Some("A")),
            dev(None, None, None),
        ];
        let mut opts = ViewOptions {
            sort: SortKey::Model,
            ..ViewOptions::default()
        };
        let idx = visible_indices(&devices, &opts);
        assert_eq!(devices[idx[0]].model.as_deref(), Some("A"));
        assert_eq!(devices[idx[1]].model.as_deref(), Some("B"));
        assert_eq!(devices[idx[2]].model, None);

        opts.dir = SortDir::Desc;
        let idx = visible_indices(&devices, &opts);
        assert_eq!(devices[idx[0]].model.as_deref(), Some("B"));
        assert_eq!(devices[idx[1]].model.as_deref(), Some("A"));
        assert_eq!(devices[idx[2]].model, None);
    }

    #[test]
    fn filter_matches_several_fields() {
        let mut d = dev(
            Some("192.168.1.64"),
            Some("a4:14:37:11:22:33"),
            Some("DS-2CD"),
        );
        d.sn = Some("SN12345".into());
        d.seen_on = vec!["以太网".into()];
        d.seen_via = SeenVia::Both;

        assert!(matches_filter(&d, ""));
        assert!(matches_filter(&d, "192.168"));
        assert!(matches_filter(&d, "a4:14"));
        assert!(matches_filter(&d, "ds-2cd"));
        assert!(matches_filter(&d, "sn123"));
        assert!(matches_filter(&d, "以太"));
        assert!(matches_filter(&d, "both"));
        assert!(!matches_filter(&d, "10.0.0.1"));
    }

    #[test]
    fn sort_key_cycles_through_all_columns() {
        let mut key = SortKey::Ip;
        for _ in 0..SortKey::ALL.len() {
            key = key.next();
        }
        assert_eq!(key, SortKey::Ip);
    }

    #[test]
    fn ipv6_global_sorts_before_link_local() {
        let mut a = dev(None, None, None);
        a.ipv6 = vec![
            Ipv6Endpoint::new("fe80::1".parse().unwrap(), Some(1)),
            Ipv6Endpoint::new("2001:db8::1".parse().unwrap(), None),
        ];
        assert_eq!(
            ip_key(&a),
            Some(IpKey::V6(u128::from(
                "2001:db8::1".parse::<std::net::Ipv6Addr>().unwrap()
            )))
        );
    }
}
