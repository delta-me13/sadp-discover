//! 导出的公共格式化：CLI 与 GUI 共用同一份字段顺序与转义规则。
//!
//! JSON 的字段名与顺序是对外契约（ADR-005，`sadp.discover/v1`），
//! 因此设备对象只在这里定义一次；两个前端各自只包装根对象（元信息）。

use std::io::{self, Write};

use serde::Serialize;

use crate::proto::Device;
use crate::proto::SeenVia;

/// JSON 里的 IPv6 端点：地址字符串 + scope + 是否全局。
#[derive(Debug, Serialize)]
pub struct DeviceJson {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sn: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mac: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ipv4: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub netmask: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gateway: Option<String>,
    pub ipv6: Vec<JsonIpv6>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ipv6_masklen: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dhcp: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub support: Option<String>,
    pub seen_via: String,
    pub seen_on: Vec<String>,
    pub first_seen_ago_ms: u64,
    pub last_seen_ago_ms: u64,
    #[serde(skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub extra: std::collections::BTreeMap<String, String>,
}

#[derive(Debug, Serialize)]
pub struct JsonIpv6 {
    pub address: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope_id: Option<u32>,
    pub global: bool,
}

impl DeviceJson {
    #[must_use]
    pub fn from_device(d: &Device) -> Self {
        Self {
            sn: d.sn.clone(),
            mac: d.mac.map(|m| m.to_string()),
            ipv4: d.ipv4.map(|i| i.to_string()),
            netmask: d.netmask.map(|i| i.to_string()),
            gateway: d.gateway.map(|i| i.to_string()),
            ipv6: d
                .ipv6
                .iter()
                .map(|e| JsonIpv6 {
                    address: e.addr.to_string(),
                    scope_id: e.scope_id,
                    global: e.global,
                })
                .collect(),
            ipv6_masklen: d.ipv6_masklen,
            dhcp: d.dhcp,
            model: d.model.clone(),
            description: d.description.clone(),
            firmware: d.firmware.clone(),
            sdk_port: d.sdk_port,
            http_port: d.http_port,
            activated: d.activated,
            reset_ability: d.reset_ability,
            analog_channels: d.analog_channels,
            digital_channels: d.digital_channels,
            support: d.support.clone(),
            seen_via: d.seen_via.as_str().to_string(),
            seen_on: d.seen_on.clone(),
            first_seen_ago_ms: d.first_seen.elapsed().as_millis() as u64,
            last_seen_ago_ms: d.last_seen.elapsed().as_millis() as u64,
            extra: d.extra.clone(),
        }
    }
}

/// CSV 行：字段顺序即表头顺序，和 `--csv` 的对外格式一致。
#[derive(Debug, Serialize)]
pub struct CsvRow {
    sn: String,
    mac: String,
    ipv4: String,
    netmask: String,
    gateway: String,
    ipv6: String,
    ipv6_masklen: String,
    dhcp: String,
    model: String,
    description: String,
    firmware: String,
    sdk_port: String,
    http_port: String,
    activated: String,
    reset_ability: String,
    analog_channels: String,
    digital_channels: String,
    support: String,
    seen_via: String,
    seen_on: String,
}

impl CsvRow {
    #[must_use]
    pub fn from_device(d: &Device) -> Self {
        let opt = |v: &Option<String>| v.clone().unwrap_or_default();
        Self {
            sn: opt(&d.sn),
            mac: d.mac.map(|m| m.to_string()).unwrap_or_default(),
            ipv4: d.ipv4.map(|i| i.to_string()).unwrap_or_default(),
            netmask: d.netmask.map(|i| i.to_string()).unwrap_or_default(),
            gateway: d.gateway.map(|i| i.to_string()).unwrap_or_default(),
            ipv6: d
                .ipv6
                .iter()
                .map(|e| e.addr.to_string())
                .collect::<Vec<_>>()
                .join(";"),
            ipv6_masklen: d.ipv6_masklen.map(|v| v.to_string()).unwrap_or_default(),
            dhcp: d.dhcp.map(|v| v.to_string()).unwrap_or_default(),
            model: opt(&d.model),
            description: opt(&d.description),
            firmware: opt(&d.firmware),
            sdk_port: d.sdk_port.map(|v| v.to_string()).unwrap_or_default(),
            http_port: d.http_port.map(|v| v.to_string()).unwrap_or_default(),
            activated: d.activated.map(|v| v.to_string()).unwrap_or_default(),
            reset_ability: d.reset_ability.map(|v| v.to_string()).unwrap_or_default(),
            analog_channels: d.analog_channels.map(|v| v.to_string()).unwrap_or_default(),
            digital_channels: d
                .digital_channels
                .map(|v| v.to_string())
                .unwrap_or_default(),
            support: opt(&d.support),
            seen_via: d.seen_via.as_str().to_string(),
            seen_on: d.seen_on.join(";"),
        }
    }
}

/// 按 CLI 的 CSV 契约写表头与数据行；两个前端都走这里。
pub fn write_csv<W: Write>(w: W, devices: &[Device]) -> io::Result<()> {
    let mut writer = csv::WriterBuilder::new().from_writer(w);
    for device in devices {
        writer
            .serialize(CsvRow::from_device(device))
            .map_err(|e| io::Error::other(e.to_string()))?;
    }
    writer.flush()
}

/// 内存里生成 CSV，供 GUI 的「另存为」使用。
pub fn csv_string(devices: &[Device]) -> io::Result<String> {
    let mut buffer = Vec::new();
    write_csv(&mut buffer, devices)?;
    String::from_utf8(buffer).map_err(|e| io::Error::other(e.to_string()))
}

/// 复制到剪贴板：一行一台，制表符分隔，粘进 Excel 或聊天窗口都不散。
#[must_use]
pub fn clipboard(devices: &[Device]) -> String {
    devices
        .iter()
        .map(|d| {
            format!(
                "{}\t{}\t{}\t{}",
                d.primary_ip(),
                d.mac.map(|m| m.to_string()).unwrap_or_else(|| "-".into()),
                d.display_model().unwrap_or("-"),
                via_label(d)
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[must_use]
pub const fn via_label(device: &Device) -> &'static str {
    match device.seen_via {
        SeenVia::V4 => "v4",
        SeenVia::V6 => "v6",
        SeenVia::Both => "both",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::proto::Ipv6Endpoint;

    fn sample() -> Device {
        let mut device = Device::fresh();
        device.sn = Some("DS-TEST-0001".to_string());
        device.model = Some("DS-2CD2T47G2-L".to_string());
        device.description = Some("a,b\"c".to_string());
        device.ipv4 = Some("192.168.1.10".parse().unwrap());
        device.mac = crate::proto::MacAddr::parse("aa:bb:cc:dd:ee:ff");
        device.dhcp = Some(false);
        device.firmware = Some("V5.7.3".to_string());
        device.ipv6 = vec![Ipv6Endpoint::new("fe80::1".parse().unwrap(), Some(7))];
        device.seen_via = SeenVia::Both;
        device.seen_on = vec!["eth0".to_string()];
        device
    }

    #[test]
    fn csv_has_the_frozen_cli_header() {
        let text = csv_string(&[sample()]).unwrap();
        let mut lines = text.lines();
        assert_eq!(
            lines.next().unwrap(),
            "sn,mac,ipv4,netmask,gateway,ipv6,ipv6_masklen,dhcp,model,description,firmware,\
             sdk_port,http_port,activated,reset_ability,analog_channels,digital_channels,\
             support,seen_via,seen_on"
        );
        let row = lines.next().unwrap();
        assert!(row.starts_with("DS-TEST-0001,aa:bb:cc:dd:ee:ff,192.168.1.10,"));
        assert!(row.contains("\"a,b\"\"c\""));
        assert!(row.ends_with(",both,eth0"), "got: {row}");
    }

    #[test]
    fn json_device_carries_contract_fields() {
        let value = serde_json::to_value(DeviceJson::from_device(&sample())).unwrap();
        assert_eq!(value["sn"], "DS-TEST-0001");
        assert_eq!(value["seen_via"], "both");
        assert_eq!(value["seen_on"][0], "eth0");
        assert_eq!(value["ipv6"][0]["address"], "fe80::1");
        assert_eq!(value["ipv6"][0]["scope_id"], 7);
        assert!(value["first_seen_ago_ms"].is_number());
    }

    #[test]
    fn clipboard_is_one_line_per_device() {
        let text = clipboard(&[sample(), sample()]);
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 2);
        assert_eq!(
            lines[0],
            "192.168.1.10\taa:bb:cc:dd:ee:ff\tDS-2CD2T47G2-L\tboth"
        );
    }

    #[test]
    fn via_label_covers_all_variants() {
        let mut device = sample();
        device.seen_via = SeenVia::V4;
        assert_eq!(via_label(&device), "v4");
        device.seen_via = SeenVia::V6;
        assert_eq!(via_label(&device), "v6");
        device.seen_via = SeenVia::Both;
        assert_eq!(via_label(&device), "both");
    }
}
