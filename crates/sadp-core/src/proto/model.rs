//! SADP 设备模型：地址、MAC、设备字段与合并规则。
//!
//! 本模块不涉及任何 IO，全部是纯数据结构与纯函数，便于单测与属性测试。

use std::collections::BTreeMap;
use std::fmt;
use std::net::{Ipv4Addr, Ipv6Addr};
use std::time::Instant;

use serde::Serialize;

/// 48 位 MAC 地址。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct MacAddr([u8; 6]);

impl MacAddr {
    #[must_use]
    pub const fn new(bytes: [u8; 6]) -> Self {
        Self(bytes)
    }

    /// 解析 `aa:bb:cc:dd:ee:ff`、`aa-bb-cc-dd-ee-ff`、`aabb.ccdd.eeff`、`aabbccddeeff`。
    #[must_use]
    pub fn parse(input: &str) -> Option<Self> {
        let s = input.trim();
        if s.is_empty() {
            return None;
        }
        let hex: String = s
            .chars()
            .filter(|c| !matches!(c, ':' | '-' | '.' | ' '))
            .collect();
        if hex.len() != 12 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
            return None;
        }
        let mut out = [0u8; 6];
        for (i, slot) in out.iter_mut().enumerate() {
            *slot = u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16).ok()?;
        }
        Some(Self(out))
    }

    #[must_use]
    pub const fn octets(&self) -> [u8; 6] {
        self.0
    }

    /// 无分隔符的 12 位小写十六进制，用于导出文件名等场景。
    #[must_use]
    pub fn to_compact(self) -> String {
        self.0.iter().map(|b| format!("{b:02x}")).collect()
    }
}

impl fmt::Display for MacAddr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
            self.0[0], self.0[1], self.0[2], self.0[3], self.0[4], self.0[5]
        )
    }
}

impl Serialize for MacAddr {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}

/// 设备上报的 IPv6 地址。
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct Ipv6Endpoint {
    pub addr: Ipv6Addr,
    /// 链路本地地址需要 scope 才能连接；来源网卡的 ifindex。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope_id: Option<u32>,
    /// 是否全局地址（非 link-local / loopback / ULA）。
    pub global: bool,
}

impl Ipv6Endpoint {
    #[must_use]
    pub fn new(addr: Ipv6Addr, scope_id: Option<u32>) -> Self {
        Self {
            addr,
            scope_id,
            global: is_global_ipv6(&addr),
        }
    }
}

#[must_use]
pub fn is_global_ipv6(addr: &Ipv6Addr) -> bool {
    let seg = addr.segments();
    let link_local = (seg[0] & 0xffc0) == 0xfe80;
    let unique_local = (seg[0] & 0xfe00) == 0xfc00;
    !addr.is_unspecified()
        && !addr.is_loopback()
        && !addr.is_multicast()
        && !link_local
        && !unique_local
        && addr.to_ipv4_mapped().is_none()
}

/// 该设备是通过哪个地址族被发现的。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub enum SeenVia {
    #[serde(rename = "v4")]
    V4,
    #[serde(rename = "v6")]
    V6,
    #[serde(rename = "both")]
    Both,
}

impl SeenVia {
    #[must_use]
    pub fn combine(self, other: Self) -> Self {
        match (self, other) {
            (Self::V4, Self::V4) => Self::V4,
            (Self::V6, Self::V6) => Self::V6,
            _ => Self::Both,
        }
    }

    /// 稳定的短标识：`v4` / `v6` / `both`，不翻译（JSON schema 与过滤都用它）。
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::V4 => "v4",
            Self::V6 => "v6",
            Self::Both => "both",
        }
    }
}

/// 去重键，形如 `sn:DS-2CD...` / `mac:a4:14:37:..` / `ip:192.168.1.64`。
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub struct DeviceKey(pub String);

impl fmt::Display for DeviceKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// 一台 SADP 设备。字段全部可缺省：不同固件上报的字段集合差异很大。
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Device {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sn: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mac: Option<MacAddr>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ipv4: Option<Ipv4Addr>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub netmask: Option<Ipv4Addr>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gateway: Option<Ipv4Addr>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub ipv6: Vec<Ipv6Endpoint>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ipv6_masklen: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dhcp: Option<bool>,
    /// `DeviceType`，型号字符串。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// `DeviceDescription`，用户可改，可能含中文与空格。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// `DSPVersion`，固件版本（字段名是历史遗留）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub firmware: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sdk_port: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub http_port: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub activated: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reset_ability: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub analog_channels: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub digital_channels: Option<u16>,
    /// 能力位串，语义不明，原样保留。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub support: Option<String>,
    /// 未知字段原样保留，便于排障与后续协议演进。
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub extra: BTreeMap<String, String>,
    #[serde(skip)]
    pub first_seen: Instant,
    #[serde(skip)]
    pub last_seen: Instant,
    #[serde(skip)]
    pub seen_via: SeenVia,
    #[serde(skip)]
    pub seen_on: Vec<String>,
}

impl Default for Device {
    fn default() -> Self {
        Self {
            sn: None,
            mac: None,
            ipv4: None,
            netmask: None,
            gateway: None,
            ipv6: Vec::new(),
            ipv6_masklen: None,
            dhcp: None,
            model: None,
            description: None,
            firmware: None,
            sdk_port: None,
            http_port: None,
            activated: None,
            reset_ability: None,
            analog_channels: None,
            digital_channels: None,
            support: None,
            extra: BTreeMap::new(),
            first_seen: Instant::now(),
            last_seen: Instant::now(),
            seen_via: SeenVia::V4,
            seen_on: Vec::new(),
        }
    }
}

impl Device {
    /// 用当前时间初始化时间戳。
    #[must_use]
    pub fn fresh() -> Self {
        Self::default()
    }

    /// 判断两条记录是否指向同一台设备。
    ///
    /// 优先级与语义：SN 唯一且稳定 → MAC 跨栈可合并 → IPv4 地址 → 全局 IPv6 地址。
    /// 空字符串一律视为缺失，避免老固件上报 `<DeviceSN></DeviceSN>` 造成误合并。
    #[must_use]
    pub fn same_device(&self, other: &Self) -> bool {
        if let (Some(a), Some(b)) = (non_empty(&self.sn), non_empty(&other.sn)) {
            return a == b;
        }
        if let (Some(a), Some(b)) = (self.mac, other.mac) {
            return a == b;
        }
        if let (Some(a), Some(b)) = (self.ipv4, other.ipv4) {
            return a == b;
        }
        self.ipv6
            .iter()
            .any(|a| a.global && other.ipv6.iter().any(|b| b.global && a.addr == b.addr))
    }

    /// 去重键：取当前最强身份。字段可能随着后续报文补全而变化。
    #[must_use]
    pub fn key(&self) -> DeviceKey {
        if let Some(sn) = non_empty(&self.sn) {
            return DeviceKey(format!("sn:{sn}"));
        }
        if let Some(mac) = self.mac {
            return DeviceKey(format!("mac:{mac}"));
        }
        if let Some(ip) = self.ipv4 {
            return DeviceKey(format!("ip:{ip}"));
        }
        if let Some(addr) = self.ipv6.iter().find(|a| a.global).map(|a| a.addr) {
            return DeviceKey(format!("ip:{addr}"));
        }
        DeviceKey("unknown".to_string())
    }

    /// 逐字段合并：新值非空则覆盖，地址集合取并集，时间戳取区间。
    /// 返回发生变化的字段名（静态串，供 UI 高亮；不做本地化）。
    pub fn merge_from(&mut self, other: &Self) -> Vec<&'static str> {
        let mut changed = Vec::new();

        macro_rules! merge_opt {
            ($field:ident, $name:literal) => {
                if other.$field.is_some() {
                    if self.$field != other.$field {
                        self.$field = other.$field.clone();
                        changed.push($name);
                    }
                }
            };
        }

        merge_opt!(sn, "sn");
        merge_opt!(mac, "mac");
        merge_opt!(ipv4, "ipv4");
        merge_opt!(netmask, "netmask");
        merge_opt!(gateway, "gateway");
        merge_opt!(ipv6_masklen, "ipv6_masklen");
        merge_opt!(dhcp, "dhcp");
        merge_opt!(model, "model");
        merge_opt!(description, "description");
        merge_opt!(firmware, "firmware");
        merge_opt!(sdk_port, "sdk_port");
        merge_opt!(http_port, "http_port");
        merge_opt!(activated, "activated");
        merge_opt!(reset_ability, "reset_ability");
        merge_opt!(analog_channels, "analog_channels");
        merge_opt!(digital_channels, "digital_channels");
        merge_opt!(support, "support");

        for ep in &other.ipv6 {
            if !self.ipv6.iter().any(|e| e.addr == ep.addr) {
                self.ipv6.push(ep.clone());
                changed.push("ipv6");
            }
        }
        self.ipv6.sort();

        for (k, v) in &other.extra {
            let replaced = self.extra.get(k) != Some(v);
            if replaced {
                self.extra.insert(k.clone(), v.clone());
                changed.push("extra");
            }
        }

        self.seen_via = self.seen_via.combine(other.seen_via);
        for iface in &other.seen_on {
            if !self.seen_on.iter().any(|i| i == iface) {
                self.seen_on.push(iface.clone());
            }
        }
        self.first_seen = self.first_seen.min(other.first_seen);
        self.last_seen = self.last_seen.max(other.last_seen);

        changed
    }

    /// 展示用型号。
    ///
    /// 部分固件把 `DeviceType` 填成数字编码（例如 `196607`），
    /// 可读型号在 `DeviceDescription` 里（例如 `DS-2DE4423DW-D/GLT/XM`）。
    /// 只有描述看起来像型号（`DS-` 前缀或含 `/`）时才替换，
    /// 避免把用户自定义名称（"Front Gate"）误当成型号。
    #[must_use]
    pub fn display_model(&self) -> Option<&str> {
        let numeric_model = self
            .model
            .as_deref()
            .is_some_and(|m| !m.is_empty() && m.chars().all(|c| c.is_ascii_digit()));
        if numeric_model
            && let Some(desc) = self.description.as_deref()
            && (desc.starts_with("DS-") || desc.contains('/'))
        {
            return Some(desc);
        }
        self.model.as_deref().or(self.description.as_deref())
    }

    /// 表格展示用的主地址：优先 IPv4，其次全局 IPv6，最后链路本地。
    #[must_use]
    pub fn primary_ip(&self) -> String {
        if let Some(ip) = self.ipv4 {
            return ip.to_string();
        }
        let pick = self
            .ipv6
            .iter()
            .find(|e| e.global)
            .or_else(|| self.ipv6.first());
        pick.map_or_else(|| "-".to_string(), |e| e.addr.to_string())
    }

    /// 是否有可用于识别或展示的身份字段。
    ///
    /// 这是「幽灵设备」的闸门：宽松解析会把端口上的杂散报文解成若干未知字段，
    /// 但没有 SN/MAC/地址的记录对用户毫无意义，必须在入库前丢掉。
    /// 链路本地 IPv6 也算身份（它至少能显示、能复制）。
    #[must_use]
    pub fn is_identifiable(&self) -> bool {
        non_empty(&self.sn).is_some()
            || self.mac.is_some()
            || self.ipv4.is_some()
            || !self.ipv6.is_empty()
    }
}

fn non_empty(value: &Option<String>) -> Option<&str> {
    value.as_deref().map(str::trim).filter(|s| !s.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dev(sn: Option<&str>, mac: Option<&str>, ip: Option<&str>) -> Device {
        Device {
            sn: sn.map(str::to_string),
            mac: mac.and_then(MacAddr::parse),
            ipv4: ip.map(|s| s.parse().unwrap()),
            ..Device::default()
        }
    }

    #[test]
    fn mac_parsing_accepts_common_formats() {
        let expected = MacAddr::new([0xa4, 0x14, 0x37, 0x11, 0x22, 0x33]);
        for input in [
            "a4:14:37:11:22:33",
            "A4-14-37-11-22-33",
            "a414.3711.2233",
            "a41437112233",
            " a4:14:37:11:22:33 ",
        ] {
            assert_eq!(MacAddr::parse(input), Some(expected), "input={input}");
        }
        assert_eq!(MacAddr::parse("a4:14:37:11:22"), None);
        assert_eq!(MacAddr::parse("zz:14:37:11:22:33"), None);
        assert_eq!(MacAddr::parse(""), None);
    }

    #[test]
    fn empty_sn_does_not_merge_distinct_devices() {
        let mut empty_sn = dev(Some(""), None, Some("192.168.1.10"));
        empty_sn.sn = Some(String::new());
        let other = dev(Some(""), None, Some("192.168.1.11"));
        assert!(!empty_sn.same_device(&other));
        assert_eq!(empty_sn.key(), DeviceKey("ip:192.168.1.10".into()));
    }

    #[test]
    fn mac_is_used_to_merge_across_families() {
        let v4 = dev(None, Some("a4:14:37:11:22:33"), Some("192.168.1.64"));
        let v6 = dev(None, Some("a4:14:37:11:22:33"), None);
        assert!(v4.same_device(&v6));
    }

    #[test]
    fn merge_keeps_richer_fields_and_unions_addresses() {
        let mut a = dev(Some("SN1"), Some("a4:14:37:11:22:33"), Some("192.168.1.64"));
        a.model = Some("DS-2CD".into());
        let mut b = Device {
            seen_via: SeenVia::V6,
            ..Device::default()
        };
        b.mac = MacAddr::parse("a4:14:37:11:22:33");
        b.firmware = Some("V5.7.0".into());
        b.ipv6
            .push(Ipv6Endpoint::new("fe80::1".parse().unwrap(), Some(3)));

        let changed = a.merge_from(&b);
        assert!(changed.contains(&"firmware"));
        assert!(changed.contains(&"ipv6"));
        assert_eq!(a.model.as_deref(), Some("DS-2CD"));
        assert_eq!(a.seen_via, SeenVia::Both);
        assert_eq!(a.ipv6.len(), 1);
    }

    #[test]
    fn key_priority_prefers_sn_then_mac_then_ip() {
        assert_eq!(
            dev(Some("SN9"), Some("a4:14:37:11:22:33"), Some("10.0.0.1")).key(),
            DeviceKey("sn:SN9".into())
        );
        assert_eq!(
            dev(None, Some("a4:14:37:11:22:33"), Some("10.0.0.1")).key(),
            DeviceKey("mac:a4:14:37:11:22:33".into())
        );
        assert_eq!(
            dev(None, None, Some("10.0.0.1")).key(),
            DeviceKey("ip:10.0.0.1".into())
        );
    }

    #[test]
    fn display_model_prefers_readable_name_over_numeric_code() {
        let mut device = Device {
            model: Some("196607".into()),
            description: Some("DS-2DE4423DW-D/GLT/XM".into()),
            ..Device::default()
        };
        assert_eq!(device.display_model(), Some("DS-2DE4423DW-D/GLT/XM"));

        device.description = Some("Front Gate".into());
        assert_eq!(device.display_model(), Some("196607"), "自定义名称不当型号");

        let mut named = Device {
            model: Some("DS-2CD2T47G2-L".into()),
            ..Device::default()
        };
        assert_eq!(named.display_model(), Some("DS-2CD2T47G2-L"));

        named.model = None;
        named.description = Some("IPCamera".into());
        assert_eq!(named.display_model(), Some("IPCamera"));

        assert_eq!(Device::default().display_model(), None);
    }

    #[test]
    fn global_ipv6_detection() {
        assert!(is_global_ipv6(&"2001:db8::1".parse().unwrap()));
        assert!(!is_global_ipv6(&"fe80::1".parse().unwrap()));
        assert!(!is_global_ipv6(&"fd00::1".parse().unwrap()));
        assert!(!is_global_ipv6(&"::".parse().unwrap()));
        assert!(!is_global_ipv6(&"::1".parse().unwrap()));
    }
}
