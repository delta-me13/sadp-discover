//! 用真实抓包样本回归解析器。
//!
//! fixtures 在仓库根 `tests/fixtures/`，脱敏后保留字段结构（含老固件嵌套、脏数据）。

use std::path::PathBuf;

use sadp_core::proto::{DecodeError, MacAddr, decode_probe_match};

fn fixture(name: &str) -> Vec<u8> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures")
        .join(name);
    std::fs::read(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

#[test]
fn typical_ipc_over_ipv4() {
    let pm = decode_probe_match(&fixture("probe_match_ipc_v4.xml")).expect("parse");
    let d = pm.device;
    assert_eq!(d.sn.as_deref(), Some("DS-2CD2T47G2-L20210618AAWRL12345678"));
    assert_eq!(d.mac.unwrap().to_string(), "a4:14:37:5f:4c:6d");
    assert_eq!(d.ipv4.unwrap().to_string(), "192.168.1.64");
    assert_eq!(d.model.as_deref(), Some("DS-2CD2T47G2-L"));
    assert_eq!(d.firmware.as_deref(), Some("V5.7.3 build 210618"));
    assert_eq!(d.activated, Some(true));
    assert_eq!(d.sdk_port, Some(8000));
    assert_eq!(d.http_port, Some(80));
    assert_eq!(d.digital_channels, Some(1));
    assert!(d.extra.is_empty());
}

#[test]
fn legacy_nvr_with_nested_device_address() {
    let d = decode_probe_match(&fixture("probe_match_nvr_legacy.xml"))
        .expect("parse")
        .device;
    assert_eq!(d.ipv4.unwrap().to_string(), "10.0.0.9");
    assert_eq!(d.netmask.unwrap().to_string(), "255.255.0.0");
    assert_eq!(d.gateway.unwrap().to_string(), "10.0.0.1");
    assert_eq!(d.sn, None, "老固件没有 DeviceSN");
    assert_eq!(d.activated, None);
    assert_eq!(d.mac.unwrap().to_string(), "44:19:b6:aa:bb:cc");
}

#[test]
fn dual_stack_with_chinese_description() {
    let d = decode_probe_match(&fixture("probe_match_dual_stack.xml"))
        .expect("parse")
        .device;
    assert_eq!(d.description.as_deref(), Some("球机-东门"));
    assert_eq!(d.ipv4.unwrap().to_string(), "192.168.1.65");
    assert_eq!(d.ipv6.len(), 2);
    assert_eq!(d.ipv6_masklen, Some(64));
    assert_eq!(d.dhcp, Some(true));
    assert_eq!(d.http_port, Some(443));
    assert!(d.ipv6.iter().any(|e| e.global));
    assert!(
        d.ipv6
            .iter()
            .any(|e| !e.global && e.addr.to_string().starts_with("fe80:"))
    );
}

#[test]
fn minimal_device_without_sn_still_usable() {
    let d = decode_probe_match(&fixture("probe_match_minimal.xml"))
        .expect("parse")
        .device;
    assert!(d.is_identifiable());
    assert_eq!(d.sn, None);
    assert_eq!(d.mac, MacAddr::parse("bc:ad:28:12:34:56"));
    assert_eq!(d.key().0, "mac:bc:ad:28:12:34:56");
}

#[test]
fn dirty_packet_with_bom_entities_and_unknown_fields() {
    let d = decode_probe_match(&fixture("probe_match_dirty.xml"))
        .expect("parse")
        .device;
    assert_eq!(
        d.description.as_deref(),
        Some("Lobby & Entrance"),
        "实体引用必须被解码"
    );
    assert_eq!(
        d.ipv4.unwrap().to_string(),
        "192.168.10.33",
        "值两侧空白要裁掉"
    );
    assert_eq!(d.mac.unwrap().to_string(), "a4:14:37:99:88:77");
    assert_eq!(d.dhcp, Some(true), "1 也要当 true");
    assert_eq!(d.activated, Some(true));
    assert_eq!(
        d.extra.get("securityversion").map(String::as_str),
        Some("2")
    );
    assert_eq!(
        d.extra.get("websocketport").map(String::as_str),
        Some("7681")
    );
}

#[test]
fn real_ds2de_capture_from_2026() {
    let d = decode_probe_match(&fixture("probe_match_real_ds2de.xml"))
        .expect("parse")
        .device;
    // DeviceType 是数字编码，真正可读的型号在 DeviceDescription。
    assert_eq!(d.model.as_deref(), Some("196607"));
    assert_eq!(d.description.as_deref(), Some("DS-2DE4423DW-D/GLT/XM"));
    assert_eq!(
        d.display_model(),
        Some("DS-2DE4423DW-D/GLT/XM"),
        "数字型号应回退到描述"
    );
    // `::` 是未配置，不能当成地址。
    assert!(d.ipv6.is_empty());
    assert_eq!(d.ipv6_masklen, Some(64), "IPv6MaskLen 大小写不影响解析");
    assert_eq!(d.mac.unwrap().to_string(), "04:ee:cd:00:00:32");
    assert_eq!(d.firmware.as_deref(), Some("V7.3 build 241203"));
    assert_eq!(d.activated, Some(true));
    assert_eq!(d.reset_ability, Some(true));
    // 未知字段要原样保留，便于排障。
    assert_eq!(
        d.extra.get("softwareversion").map(String::as_str),
        Some("V5.8.2build 241217")
    );
    assert_eq!(
        d.extra.get("hcplatformenable").map(String::as_str),
        Some("flase")
    );
}

#[test]
fn garbage_does_not_parse_as_device() {
    {
        let name = "probe_match_ipc_v4.xml";
        let mut bytes = fixture(name);
        bytes.truncate(30);
        // 截断的 XML 要么失败，要么解析出无法识别的设备，但绝不能 panic。
        let _ = decode_probe_match(&bytes);
    }
    assert!(matches!(
        decode_probe_match(b"not xml at all"),
        Err(DecodeError::NotProbeMatch)
    ));
}
