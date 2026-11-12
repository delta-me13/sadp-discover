//! 导出与复制：字段语义与 CLI 的 JSON/CSV 输出保持一致，方便脚本复用。

use sadp_core::Device;

/// JSON：schema 为 sadp.discover/v1，额外带 generated_at 便于留档。
pub fn json(devices: &[Device]) -> serde_json::Result<String> {
    let value = serde_json::json!({
        "schema": sadp_core::JSON_SCHEMA,
        "tool": "sadp-discover",
        "frontend": "gui",
        "version": sadp_core::VERSION,
        "generated_at": now_rfc3339(),
        "count": devices.len(),
        "devices": devices,
    });
    serde_json::to_string_pretty(&value)
}

/// CSV：表头固定英文，字段与 CLI 的 --csv 一致。
pub fn csv(devices: &[Device]) -> String {
    let mut out = String::from(
        "ipv4,mac,model,sn,description,firmware,netmask,gateway,dhcp,sdk_port,http_port,ipv6,via\n",
    );
    for d in devices {
        let cells = [
            d.ipv4.map(|ip| ip.to_string()).unwrap_or_default(),
            d.mac.map(|m| m.to_string()).unwrap_or_default(),
            d.display_model().unwrap_or_default().to_string(),
            d.sn.clone().unwrap_or_default(),
            d.description.clone().unwrap_or_default(),
            d.firmware.clone().unwrap_or_default(),
            d.netmask.map(|ip| ip.to_string()).unwrap_or_default(),
            d.gateway.map(|ip| ip.to_string()).unwrap_or_default(),
            d.dhcp.map(|v| v.to_string()).unwrap_or_default(),
            d.sdk_port.map(|p| p.to_string()).unwrap_or_default(),
            d.http_port.map(|p| p.to_string()).unwrap_or_default(),
            d.ipv6
                .iter()
                .map(|e| e.addr.to_string())
                .collect::<Vec<_>>()
                .join(" "),
            via_label(d).to_string(),
        ];
        let line: Vec<String> = cells.iter().map(|c| csv_cell(c)).collect();
        out.push_str(&line.join(","));
        out.push('\n');
    }
    out
}

/// 复制到剪贴板：一行一台，制表符分隔，粘进 Excel 或聊天窗口都不散。
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

pub fn via_label(device: &Device) -> &'static str {
    match device.seen_via {
        sadp_core::SeenVia::V4 => "v4",
        sadp_core::SeenVia::V6 => "v6",
        sadp_core::SeenVia::Both => "both",
    }
}

fn csv_cell(value: &str) -> String {
    if value.contains([',', '"', '\n']) {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value.to_string()
    }
}

fn now_rfc3339() -> String {
    time::OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_else(|_| String::new())
}

#[cfg(test)]
mod tests {
    use super::*;
    use sadp_core::Device;

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
    fn csv_has_stable_header_and_row() {
        let text = csv(&[sample()]);
        let mut lines = text.lines();
        assert_eq!(
            lines.next().unwrap(),
            "ipv4,mac,model,sn,description,firmware,netmask,gateway,dhcp,sdk_port,http_port,ipv6,via"
        );
        let row = lines.next().unwrap();
        assert!(row.starts_with("192.168.1.10,aa:bb:cc:dd:ee:ff,DS-2CD2T47G2-L,DS-TEST-0001,"));
        assert!(row.ends_with(",v4"));
        assert_eq!(lines.count(), 0);
    }

    #[test]
    fn csv_quotes_commas_and_quotes() {
        let mut device = sample();
        device.description = Some("a,b\"c".to_string());
        let text = csv(&[device]);
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
    fn json_carries_schema_and_count() {
        let text = json(&[sample()]).unwrap();
        let value: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(value["schema"], sadp_core::JSON_SCHEMA);
        assert_eq!(value["count"], 1);
        assert_eq!(value["frontend"], "gui");
        assert_eq!(value["devices"][0]["sn"], "DS-TEST-0001");
    }
}
