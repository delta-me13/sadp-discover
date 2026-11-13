//! 网卡枚举与筛选。
//!
//! Windows 上 Hyper-V、VPN、WSL、蓝牙虚拟网卡会让「只发默认路由」的实现漏设备，
//! 因此这里枚举**全部**网卡，由上层逐网卡发包；是否跳过回环/虚拟网卡交给 `InterfaceSelector`。

use std::collections::BTreeMap;
use std::net::{Ipv4Addr, Ipv6Addr};

use crate::{InterfaceInfo, NetError};

/// 网卡筛选策略。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum InterfaceSelector {
    /// 全部可用网卡（默认跳过回环，除非只有回环）。
    #[default]
    Auto,
    /// 用户显式指定的网卡：名称、索引或地址字符串任一种匹配即可。
    Only(Vec<String>),
    /// 全部网卡，含回环与未启用适配器（`interfaces` 诊断命令用）。
    All,
}

impl InterfaceSelector {
    #[must_use]
    pub fn is_auto(&self) -> bool {
        matches!(self, Self::Auto)
    }

    #[must_use]
    pub fn is_all(&self) -> bool {
        matches!(self, Self::All)
    }

    #[must_use]
    pub fn only(names: Vec<String>) -> Self {
        if names.is_empty() {
            Self::Auto
        } else {
            Self::Only(names)
        }
    }
}

impl std::str::FromStr for InterfaceSelector {
    type Err = std::convert::Infallible;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let items: Vec<String> = s
            .split([',', ';'])
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .map(str::to_string)
            .collect();
        Ok(Self::only(items))
    }
}

/// 枚举本机网卡并按适配器聚合 v4/v6 地址。
pub fn enumerate_interfaces(selector: &InterfaceSelector) -> Result<Vec<InterfaceInfo>, NetError> {
    let raw =
        if_addrs::get_if_addrs().map_err(|e| NetError::Io(format!("enumerate interfaces: {e}")))?;

    let mut by_name: BTreeMap<String, InterfaceInfo> = BTreeMap::new();
    for entry in raw {
        // 物理上不可用的网卡（网线未插、适配器禁用）没有发送价值；
        // Down 但仍有地址的虚拟网卡（VPN 未连接）保留，交由逐网卡发送自然失败。
        if matches!(
            entry.oper_status,
            if_addrs::IfOperStatus::NotPresent | if_addrs::IfOperStatus::Testing
        ) {
            continue;
        }
        let slot = by_name
            .entry(entry.name.clone())
            .or_insert_with(|| InterfaceInfo {
                name: entry.name.clone(),
                index: entry.index.unwrap_or(0),
                ipv4: None,
                netmask: None,
                broadcast: None,
                ipv6: Vec::new(),
                loopback: false,
            });
        if let Some(index) = entry.index {
            slot.index = index;
        }
        match entry.addr {
            if_addrs::IfAddr::V4(v4) => {
                // 同一网卡可能有多个 v4 地址，优先保留第一个非 169.254/16 的。
                let is_link_local = v4.ip.octets()[0] == 169 && v4.ip.octets()[1] == 254;
                let should_replace = slot.ipv4.is_none()
                    || (is_link_local
                        && slot.ipv4.is_some_and(|cur| {
                            !(cur.octets()[0] == 169 && cur.octets()[1] == 254)
                        }));
                if should_replace {
                    slot.ipv4 = Some(v4.ip);
                    slot.netmask = Some(v4.netmask);
                }
                // /32（如 Tailscale）会自报「广播地址 = 本机地址」，没有意义，丢掉。
                slot.broadcast = v4
                    .broadcast
                    .or_else(|| broadcast_of(v4.ip, v4.netmask))
                    .filter(|b| *b != v4.ip);
            }
            if_addrs::IfAddr::V6(v6) => {
                if !v6.ip.is_unspecified() && !slot.ipv6.contains(&v6.ip) {
                    slot.ipv6.push(v6.ip);
                }
            }
        }
    }

    let mut out: Vec<InterfaceInfo> = by_name
        .into_values()
        .map(|mut i| {
            i.loopback = is_loopback(&i);
            i.ipv6.sort();
            i
        })
        .filter(|i| i.ipv4.is_some() || !i.ipv6.is_empty())
        .collect();

    out.sort_by(|a, b| {
        a.loopback
            .cmp(&b.loopback)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });

    match selector {
        InterfaceSelector::All => Ok(out),
        InterfaceSelector::Auto => {
            let non_loopback: Vec<_> = out.iter().filter(|i| !i.loopback).cloned().collect();
            if non_loopback.is_empty() {
                Ok(out)
            } else {
                Ok(non_loopback)
            }
        }
        InterfaceSelector::Only(wanted) => {
            let filtered: Vec<_> = out
                .into_iter()
                .filter(|i| matches_selector(i, wanted))
                .collect();
            if filtered.is_empty() {
                Err(NetError::NoInterface(format!(
                    "no interface matched {:?}",
                    wanted
                )))
            } else {
                Ok(filtered)
            }
        }
    }
}

fn matches_selector(iface: &InterfaceInfo, wanted: &[String]) -> bool {
    wanted.iter().any(|w| {
        let w = w.trim();
        if w.is_empty() {
            return false;
        }
        if w.eq_ignore_ascii_case(&iface.name) {
            return true;
        }
        // 纯数字只按索引解释：此前 `--interface 1` 先命中索引 1（回环），
        // 又会继续落到地址前缀分支，把 192.168.*、10.*、100.* 全部选中，
        // 于是「只扫回环」实际扫了全部网卡，真机测试时暴露。
        if let Ok(idx) = w.parse::<u32>() {
            return idx == iface.index;
        }
        if let Ok(ip) = w.parse::<Ipv4Addr>() {
            return iface.ipv4 == Some(ip);
        }
        if let Ok(ip) = w.parse::<Ipv6Addr>() {
            return iface.ipv6.contains(&ip);
        }
        // 非完整地址才允许前缀匹配，便于 `--interface 192.168.1.` 这样的写法。
        iface.ipv4.is_some_and(|ip| ip.to_string().starts_with(w))
    })
}

fn is_loopback(iface: &InterfaceInfo) -> bool {
    let name = iface.name.to_lowercase();
    let by_name = name.contains("loopback") || name == "lo" || name == "lo0";
    let by_addr = iface.ipv4.is_some_and(|ip| ip.is_loopback())
        && iface.ipv6.iter().all(Ipv6Addr::is_loopback);
    by_name || by_addr
}

/// 由地址与掩码推算子网定向广播地址；`/31`、`/32` 无广播地址。
#[must_use]
pub fn broadcast_of(ip: Ipv4Addr, mask: Ipv4Addr) -> Option<Ipv4Addr> {
    let m = u32::from(mask);
    let host_bits = !m;
    if host_bits <= 1 {
        return None;
    }
    Some(Ipv4Addr::from(u32::from(ip) | host_bits))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn iface(name: &str, index: u32, ip: &str, mask: &str) -> InterfaceInfo {
        InterfaceInfo {
            name: name.into(),
            index,
            ipv4: Some(ip.parse().unwrap()),
            netmask: Some(mask.parse().unwrap()),
            broadcast: broadcast_of(ip.parse().unwrap(), mask.parse().unwrap()),
            ipv6: vec![],
            loopback: false,
        }
    }

    #[test]
    fn broadcast_address_is_computed_from_mask() {
        assert_eq!(
            broadcast_of(
                Ipv4Addr::new(192, 168, 1, 10),
                Ipv4Addr::new(255, 255, 255, 0)
            ),
            Some(Ipv4Addr::new(192, 168, 1, 255))
        );
        assert_eq!(
            broadcast_of(
                Ipv4Addr::new(10, 0, 0, 1),
                Ipv4Addr::new(255, 255, 255, 252)
            ),
            Some(Ipv4Addr::new(10, 0, 0, 3))
        );
        assert_eq!(
            broadcast_of(
                Ipv4Addr::new(10, 0, 0, 1),
                Ipv4Addr::new(255, 255, 255, 255)
            ),
            None
        );
    }

    #[test]
    fn selector_matches_name_index_and_address() {
        let i = iface("Ethernet 2", 7, "192.168.1.10", "255.255.255.0");
        assert!(matches_selector(&i, &["ethernet 2".into()]));
        assert!(matches_selector(&i, &["7".into()]));
        assert!(matches_selector(&i, &["192.168.1.10".into()]));
        assert!(matches_selector(&i, &["192.168.1.".into()]));
        assert!(!matches_selector(&i, &["10.0.0.1".into()]));
        // 纯数字是索引，不是地址前缀：索引 1 的网卡不应选中 192.168.1.10。
        assert!(!matches_selector(&i, &["1".into()]));
        assert!(matches_selector(&i, &["7".into()]));
    }

    #[test]
    fn subnet_membership_check() {
        let i = iface("eth0", 1, "192.168.1.10", "255.255.255.0");
        assert!(i.contains_v4(Ipv4Addr::new(192, 168, 1, 200)));
        assert!(!i.contains_v4(Ipv4Addr::new(192, 168, 2, 1)));
    }

    #[test]
    fn selector_parsing_from_cli_string() {
        assert_eq!(
            "a,b ; c".parse::<InterfaceSelector>().unwrap(),
            InterfaceSelector::Only(vec!["a".into(), "b".into(), "c".into()])
        );
        assert_eq!(
            "".parse::<InterfaceSelector>().unwrap(),
            InterfaceSelector::Auto
        );
    }
}
