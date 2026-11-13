//! GUI 导出：设备字段来自 `sadp_core::export`，与 CLI 的 JSON/CSV 完全一致；
//! 根对象额外带 `frontend`，方便区分手工导出与 `--json`。

use std::time::Duration;

use sadp_core::export::{self, DeviceJson};
use sadp_core::{Device, Family, JSON_SCHEMA};

/// 导出时写入根对象的扫描元信息。
pub struct Meta<'a> {
    pub port: u16,
    pub families: &'a [Family],
    pub elapsed: Duration,
    pub rounds: u64,
}

/// JSON：schema 为 `sadp.discover/v1`，设备对象的字段顺序与 CLI 相同。
pub fn json(devices: &[Device], meta: &Meta<'_>) -> serde_json::Result<String> {
    let value = serde_json::json!({
        "schema": JSON_SCHEMA,
        "tool": "sadp-discover",
        "frontend": "gui",
        "version": sadp_core::VERSION,
        "generated_at": now_rfc3339(),
        "elapsed_ms": meta.elapsed.as_millis() as u64,
        "port": meta.port,
        "families": meta.families.iter().map(|f| f.as_str()).collect::<Vec<_>>(),
        "rounds": meta.rounds,
        "count": devices.len(),
        "devices": devices.iter().map(DeviceJson::from_device).collect::<Vec<_>>(),
    });
    serde_json::to_string_pretty(&value)
}

/// CSV：表头与 `sadp-discover --csv` 完全一致。
pub fn csv(devices: &[Device]) -> std::io::Result<String> {
    export::csv_string(devices)
}

/// 复制到剪贴板：一行一台，制表符分隔。
#[must_use]
pub fn clipboard(devices: &[Device]) -> String {
    export::clipboard(devices)
}

fn now_rfc3339() -> String {
    time::OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_else(|_| String::new())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Device {
        let mut device = Device::fresh();
        device.sn = Some("DS-TEST-0001".to_string());
        device.model = Some("DS-2CD2T47G2-L".to_string());
        device.ipv4 = Some("192.168.1.10".parse().unwrap());
        device.mac = sadp_core::MacAddr::parse("aa:bb:cc:dd:ee:ff");
        device.dhcp = Some(false);
        device.firmware = Some("V5.7.3".to_string());
        device
    }

    #[test]
    fn csv_uses_the_cli_header() {
        let text = csv(&[sample()]).unwrap();
        let mut lines = text.lines();
        assert_eq!(
            lines.next().unwrap(),
            "sn,mac,ipv4,netmask,gateway,ipv6,ipv6_masklen,dhcp,model,description,firmware,\
             sdk_port,http_port,activated,reset_ability,analog_channels,digital_channels,\
             support,seen_via,seen_on"
        );
        let row = lines.next().unwrap();
        assert!(row.starts_with("DS-TEST-0001,aa:bb:cc:dd:ee:ff,192.168.1.10,"));
        assert!(row.ends_with(",v4,"), "got: {row}");
        assert_eq!(lines.count(), 0);
    }

    #[test]
    fn csv_quotes_commas_and_quotes() {
        let mut device = sample();
        device.description = Some("a,b\"c".to_string());
        let text = csv(&[device]).unwrap();
        assert!(text.contains("\"a,b\"\"c\""), "got: {text}");
    }

    #[test]
    fn clipboard_uses_one_line_per_device_with_tabs() {
        let text = clipboard(&[sample(), sample()]);
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 2);
        assert!(lines[0].starts_with("192.168.1.10\taa:bb:cc:dd:ee:ff\tDS-2CD2T47G2-L\tv4"));
    }

    #[test]
    fn json_carries_schema_metadata_and_shared_device_fields() {
        let meta = Meta {
            port: 37020,
            families: &[Family::V4, Family::V6],
            elapsed: Duration::from_millis(1200),
            rounds: 3,
        };
        let text = json(&[sample()], &meta).unwrap();
        let value: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(value["schema"], sadp_core::JSON_SCHEMA);
        assert_eq!(value["frontend"], "gui");
        assert_eq!(value["count"], 1);
        assert_eq!(value["port"], 37020);
        assert_eq!(value["families"][0], "v4");
        assert_eq!(value["rounds"], 3);
        assert_eq!(value["elapsed_ms"], 1200);
        assert_eq!(value["devices"][0]["sn"], "DS-TEST-0001");
        assert_eq!(value["devices"][0]["seen_via"], "v4");
        assert!(value["devices"][0]["first_seen_ago_ms"].is_number());
    }
}
