//! `ProbeMatch` 宽松解析。
//!
//! 设计要点（对应架构文档 §2.3）：
//! - 不假设 XML 声明、BOM、字段顺序、大小写、命名空间；
//! - 未知字段进 `extra` 保留，不因未知字段失败；
//! - 单个字段值有长度上限，报文整体由传输层按 64 KiB 截断；
//! - 解析器只做纯字符串处理，不 panic（fuzz 目标见 `fuzz/`）。

use std::collections::BTreeMap;
use std::net::{Ipv4Addr, Ipv6Addr};

use quick_xml::Reader;
use quick_xml::events::Event;

use super::model::{Device, Ipv6Endpoint, MacAddr};

/// 单个字段值上限，防止伪造的超长字段撑爆终端与导出。
const MAX_VALUE_LEN: usize = 1024;
/// 字段数量上限。
const MAX_FIELDS: usize = 256;
/// 解析器输入上限（传输层已按 64 KiB 截断，这里再兜一层）。
const MAX_INPUT_LEN: usize = 64 * 1024;

#[derive(Debug, thiserror::Error)]
pub enum DecodeError {
    #[error("empty datagram")]
    Empty,
    #[error("not a SADP ProbeMatch datagram")]
    NotProbeMatch,
    #[error("malformed XML: {0}")]
    Xml(String),
}

/// 一条已解码的应答。
#[derive(Clone, Debug)]
pub struct ProbeMatch {
    pub device: Device,
    /// `<Types>` 原值，正常为 `inquiry`；异常值可用于排障。
    pub types: Option<String>,
    /// 原始报文，`--save-raw` 时落盘。
    pub raw: Vec<u8>,
}

impl ProbeMatch {
    #[must_use]
    pub fn into_device(self) -> Device {
        self.device
    }
}

/// 数据报里是否出现 `ProbeMatch` 根标签（大小写不敏感）。
///
/// 用来区分「设备没应答」与「本机根本没收到任何 SADP 响应」（权限/防火墙拦截）。
/// 这里只看标签，不做完整解析，代价极低。
#[must_use]
pub fn looks_like_probe_match(bytes: &[u8]) -> bool {
    const NEEDLE: &[u8] = b"<probematch";
    bytes
        .windows(NEEDLE.len())
        .any(|window| window.eq_ignore_ascii_case(NEEDLE))
}

/// 解析一条 `ProbeMatch`。输入可以是任意字节。
pub fn decode_probe_match(bytes: &[u8]) -> Result<ProbeMatch, DecodeError> {
    if bytes.is_empty() {
        return Err(DecodeError::Empty);
    }
    let slice = &bytes[..bytes.len().min(MAX_INPUT_LEN)];
    // 先做有损 UTF-8 转换：部分设备会混入 GBK 注释或截断的多字节字符，
    // 为这类报文直接失败会丢掉整台设备，得不偿失。
    let text = String::from_utf8_lossy(slice);
    let text = text.trim_start_matches('\u{feff}');

    let mut reader = Reader::from_str(text);
    let config = reader.config_mut();
    config.check_end_names = false;
    config.allow_unmatched_ends = true;
    config.expand_empty_elements = false;
    config.trim_text_start = false;
    config.trim_text_end = false;

    let mut stack: Vec<String> = Vec::new();
    let mut fields: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut pending = String::new();
    let mut pending_parent: Option<String> = None;
    let mut saw_probe_match = false;
    let mut saw_any_field = false;

    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) => {
                flush_text(
                    &mut fields,
                    &mut pending_parent,
                    &mut pending,
                    &mut saw_any_field,
                );
                let name = local_name(e.name().as_ref());
                if stack.is_empty() && name == "probematch" {
                    saw_probe_match = true;
                }
                push_depth(&mut stack, name);
            }
            Ok(Event::Empty(e)) => {
                flush_text(
                    &mut fields,
                    &mut pending_parent,
                    &mut pending,
                    &mut saw_any_field,
                );
                let name = local_name(e.name().as_ref());
                if stack.is_empty() && name == "probematch" {
                    saw_probe_match = true;
                }
            }
            Ok(Event::Text(e)) => {
                append_text(
                    &stack,
                    &e.xml10_content(),
                    &mut pending,
                    &mut pending_parent,
                );
            }
            Ok(Event::GeneralRef(e)) => {
                let decoded = decode_entity_ref(&e);
                append_text(&stack, &decoded, &mut pending, &mut pending_parent);
            }
            Ok(Event::CData(e)) => {
                append_text(
                    &stack,
                    &e.xml10_content(),
                    &mut pending,
                    &mut pending_parent,
                );
            }
            Ok(Event::End(_)) => {
                flush_text(
                    &mut fields,
                    &mut pending_parent,
                    &mut pending,
                    &mut saw_any_field,
                );
                stack.pop();
            }
            Ok(Event::Eof) => {
                flush_text(
                    &mut fields,
                    &mut pending_parent,
                    &mut pending,
                    &mut saw_any_field,
                );
                break;
            }
            Ok(_) => {}
            Err(e) => return Err(DecodeError::Xml(e.to_string())),
        }
    }

    if !saw_probe_match && !saw_any_field {
        return Err(DecodeError::NotProbeMatch);
    }

    let types = first(&fields, "types").map(str::to_string);
    // `Types` 明确不是 inquiry 说明是别的交互（如写操作的回包），不当作发现结果。
    if let Some(t) = &types {
        let t = t.trim();
        if !t.is_empty() && !t.eq_ignore_ascii_case("inquiry") {
            return Err(DecodeError::NotProbeMatch);
        }
    }

    let device = build_device(&fields);
    // 宽松解析的代价：端口上的杂散二进制/半截 XML 也可能被切成「未知字段」。
    // 只要没有任何身份字段（SN/MAC/地址），就一律不当作设备，
    // 否则 TUI 里会出现一排只有 "-" 的幽灵行。
    if !device.is_identifiable() {
        return Err(DecodeError::NotProbeMatch);
    }

    Ok(ProbeMatch {
        device,
        types,
        raw: slice.to_vec(),
    })
}

/// 把当前元素累积的文本落到字段表里。
fn flush_text(
    fields: &mut BTreeMap<String, Vec<String>>,
    parent: &mut Option<String>,
    buf: &mut String,
    saw_any: &mut bool,
) {
    let value = buf.trim();
    if !value.is_empty()
        && let Some(p) = parent.as_ref()
        && !p.is_empty()
        && fields.len() < MAX_FIELDS
    {
        fields
            .entry(p.clone())
            .or_default()
            .push(cap(value, MAX_VALUE_LEN));
        *saw_any = true;
    }
    buf.clear();
    *parent = None;
}

fn append_text(
    stack: &[String],
    text: &str,
    pending: &mut String,
    pending_parent: &mut Option<String>,
) {
    if text.is_empty() {
        return;
    }
    if let Some(parent) = stack.last() {
        if parent.is_empty() {
            return;
        }
        pending_parent.get_or_insert_with(|| parent.clone());
        if pending.len() < MAX_VALUE_LEN * 4 {
            pending.push_str(text);
        }
    }
}

fn push_depth(stack: &mut Vec<String>, name: String) {
    // 恶意报文可能构造极深嵌套；超过上限后只保留占位，避免栈爆。
    if stack.len() >= 64 {
        stack.push(String::new());
    } else {
        stack.push(name);
    }
}

/// 解码 `&amp;` / `&#38;` / `&#x26;` 这类实体引用；未知实体原样保留。
fn decode_entity_ref(reference: &quick_xml::events::BytesRef<'_>) -> String {
    let name: &str = reference;
    if let Some(hex) = name.strip_prefix("#x").or_else(|| name.strip_prefix("#X")) {
        return u32::from_str_radix(hex, 16)
            .ok()
            .and_then(char::from_u32)
            .map_or_else(|| format!("&{name};"), |c| c.to_string());
    }
    if let Some(dec) = name.strip_prefix('#') {
        return dec
            .parse::<u32>()
            .ok()
            .and_then(char::from_u32)
            .map_or_else(|| format!("&{name};"), |c| c.to_string());
    }
    match name {
        "amp" => "&".to_string(),
        "lt" => "<".to_string(),
        "gt" => ">".to_string(),
        "quot" => "\"".to_string(),
        "apos" => "'".to_string(),
        other => format!("&{other};"),
    }
}

fn build_device(fields: &BTreeMap<String, Vec<String>>) -> Device {
    let mut device = Device::fresh();

    device.sn = first(fields, "devicesn").map(str::to_string);
    device.mac = first(fields, "mac").and_then(MacAddr::parse);
    device.model = first(fields, "devicetype").map(str::to_string);
    device.description = first(fields, "devicedescription").map(str::to_string);
    device.firmware = first(fields, "dspversion").map(str::to_string);
    device.support = first(fields, "support").map(str::to_string);

    // 网络字段同时兼容顶层与老固件的 <DeviceAddress> 嵌套写法：
    // 解析阶段已按「最后一级元素名」拍平，两种形态自然等价。
    // `0.0.0.0` 视为「未配置」而不是一个地址，否则表格里会出现 0.0.0.0 的假行。
    device.ipv4 = parse_ipv4(first(fields, "ipv4address"));
    device.netmask = parse_ipv4(first(fields, "ipv4subnetmask"));
    device.gateway = parse_ipv4(first(fields, "ipv4gateway"));
    device.ipv6_masklen = first(fields, "ipv6masklen").and_then(parse_u8);
    device.dhcp = first(fields, "dhcp").and_then(parse_bool);
    device.sdk_port = first(fields, "commandport").and_then(parse_u16);
    device.http_port = first(fields, "httpport").and_then(parse_u16);
    device.activated = first(fields, "activated").and_then(parse_bool);
    device.reset_ability = first(fields, "passwordresetability").and_then(parse_bool);
    device.analog_channels = first(fields, "analogchannelnum").and_then(parse_u16);
    device.digital_channels = first(fields, "digitalchannelnum").and_then(parse_u16);

    for raw in fields.get("ipv6address").map(Vec::as_slice).unwrap_or(&[]) {
        for part in split_address_list(raw) {
            if let Some(ep) = parse_ipv6(part)
                && !device.ipv6.iter().any(|e| e.addr == ep.addr)
            {
                device.ipv6.push(ep);
            }
        }
    }
    device.ipv6.sort();

    for (key, values) in fields {
        if is_known(key) {
            continue;
        }
        let joined = values.join(" | ");
        device
            .extra
            .insert(key.clone(), cap(&joined, MAX_VALUE_LEN));
    }

    device
}

fn is_known(key: &str) -> bool {
    matches!(
        key,
        "probematch"
            | "uuid"
            | "types"
            | "devicetype"
            | "devicedescription"
            | "devicesn"
            | "commandport"
            | "httpport"
            | "mac"
            | "ipv4address"
            | "ipv4subnetmask"
            | "ipv4gateway"
            | "ipv6address"
            | "ipv6masklen"
            | "ipv6gateway"
            | "dhcp"
            | "dspversion"
            | "activated"
            | "passwordresetability"
            | "analogchannelnum"
            | "digitalchannelnum"
            | "support"
            | "deviceaddress"
    )
}

fn first<'a>(fields: &'a BTreeMap<String, Vec<String>>, key: &str) -> Option<&'a str> {
    fields
        .get(key)
        .and_then(|v| v.first())
        .map(String::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
}

/// 取本地名并转小写：`ns:IPv4Address` → `ipv4address`。
fn local_name(raw: &str) -> String {
    let local = raw.rsplit(':').next().unwrap_or(raw);
    local.trim().to_ascii_lowercase()
}

fn cap(value: &str, max: usize) -> String {
    if value.len() <= max {
        return value.to_string();
    }
    let mut end = max;
    while end > 0 && !value.is_char_boundary(end) {
        end -= 1;
    }
    value[..end].to_string()
}

fn parse_ipv4(value: Option<&str>) -> Option<Ipv4Addr> {
    value
        .and_then(|s| s.parse::<Ipv4Addr>().ok())
        .filter(|ip| !ip.is_unspecified())
}

fn parse_bool(value: &str) -> Option<bool> {
    match value.trim().to_ascii_lowercase().as_str() {
        "true" | "1" | "yes" | "on" => Some(true),
        "false" | "0" | "no" | "off" => Some(false),
        _ => None,
    }
}

fn parse_u16(value: &str) -> Option<u16> {
    value.trim().parse().ok()
}

fn parse_u8(value: &str) -> Option<u8> {
    value.trim().parse().ok()
}

fn parse_ipv6(value: &str) -> Option<Ipv6Endpoint> {
    let s = value.trim();
    if s.is_empty() {
        return None;
    }
    let s = s.trim_start_matches('[').trim_end_matches(']');
    let (addr_part, scope) = match s.split_once('%') {
        Some((addr, scope)) => (addr, scope.parse::<u32>().ok()),
        None => (s, None),
    };
    let addr: Ipv6Addr = addr_part.parse().ok()?;
    if addr.is_unspecified() {
        return None;
    }
    Some(Ipv6Endpoint::new(addr, scope))
}

/// 部分固件把多个地址塞进一个元素，用 `;` / `,` / 空格分隔。
fn split_address_list(value: &str) -> Vec<&str> {
    if value.parse::<Ipv6Addr>().is_ok() {
        return vec![value];
    }
    value
        .split([';', ',', ' ', '\t'])
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const BASIC: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<ProbeMatch>
  <Uuid>0f8fad5b-d9cb-469f-a165-70867728950e</Uuid>
  <Types>inquiry</Types>
  <DeviceType>DS-2CD2T47G2-L</DeviceType>
  <DeviceDescription>Front Gate</DeviceDescription>
  <DeviceSN>DS-2CD2T47G2-L20200101AAWRL12345678</DeviceSN>
  <CommandPort>8000</CommandPort>
  <HttpPort>80</HttpPort>
  <MAC>a4:14:37:11:22:33</MAC>
  <IPv4Address>192.168.1.64</IPv4Address>
  <IPv4SubnetMask>255.255.255.0</IPv4SubnetMask>
  <IPv4Gateway>192.168.1.1</IPv4Gateway>
  <DHCP>false</DHCP>
  <DSPVersion>V5.7.3 build 200101</DSPVersion>
  <Activated>true</Activated>
  <PasswordResetAbility>true</PasswordResetAbility>
  <AnalogChannelNum>0</AnalogChannelNum>
  <DigitalChannelNum>1</DigitalChannelNum>
  <Support>0x00000000</Support>
</ProbeMatch>"#;

    #[test]
    fn parses_typical_ipc() {
        let pm = decode_probe_match(BASIC.as_bytes()).expect("must parse");
        let d = pm.device;
        assert_eq!(d.sn.as_deref(), Some("DS-2CD2T47G2-L20200101AAWRL12345678"));
        assert_eq!(d.mac.unwrap().to_string(), "a4:14:37:11:22:33");
        assert_eq!(d.ipv4.unwrap().to_string(), "192.168.1.64");
        assert_eq!(d.netmask.unwrap().to_string(), "255.255.255.0");
        assert_eq!(d.model.as_deref(), Some("DS-2CD2T47G2-L"));
        assert_eq!(d.description.as_deref(), Some("Front Gate"));
        assert_eq!(d.sdk_port, Some(8000));
        assert_eq!(d.http_port, Some(80));
        assert_eq!(d.dhcp, Some(false));
        assert_eq!(d.activated, Some(true));
        assert_eq!(d.digital_channels, Some(1));
        assert_eq!(pm.types.as_deref(), Some("inquiry"));
    }

    #[test]
    fn parses_nested_device_address_from_old_firmware() {
        let xml = r#"<?xml version="1.0"?>
<ProbeMatch>
  <Uuid>abc</Uuid>
  <Types>inquiry</Types>
  <MAC>44:19:b6:aa:bb:cc</MAC>
  <DeviceAddress>
    <IPv4Address>10.0.0.9</IPv4Address>
    <IPv4SubnetMask>255.255.0.0</IPv4SubnetMask>
    <IPv4Gateway>10.0.0.1</IPv4Gateway>
  </DeviceAddress>
  <DeviceType>DS-7608N-K2</DeviceType>
</ProbeMatch>"#;
        let d = decode_probe_match(xml.as_bytes()).unwrap().device;
        assert_eq!(d.ipv4.unwrap().to_string(), "10.0.0.9");
        assert_eq!(d.netmask.unwrap().to_string(), "255.255.0.0");
        assert_eq!(d.sn, None);
        assert_eq!(d.mac.unwrap().to_string(), "44:19:b6:aa:bb:cc");
    }

    #[test]
    fn parses_dual_stack_with_multiple_ipv6() {
        let xml = r#"<ProbeMatch>
  <Types>inquiry</Types>
  <DeviceSN>SN-DUAL</DeviceSN>
  <MAC>a4-14-37-11-22-33</MAC>
  <IPv4Address>192.168.1.65</IPv4Address>
  <IPv6Address>fe80::21b:2bff:fe3c:4d5e</IPv6Address>
  <IPv6Address>2001:db8::65</IPv6Address>
  <IPv6Masklen>64</IPv6Masklen>
  <IPv6Gateway>2001:db8::1</IPv6Gateway>
  <DHCP>1</DHCP>
</ProbeMatch>"#;
        let d = decode_probe_match(xml.as_bytes()).unwrap().device;
        assert_eq!(d.ipv6.len(), 2);
        assert_eq!(d.ipv6_masklen, Some(64));
        assert_eq!(d.dhcp, Some(true));
        assert!(d.ipv6.iter().any(|e| e.global));
        assert!(d.ipv6.iter().any(|e| !e.global));
        assert_eq!(d.ipv4.unwrap().to_string(), "192.168.1.65");
    }

    #[test]
    fn tolerates_bom_comments_namespaces_and_unknown_fields() {
        let xml = "\u{feff}<?xml version=\"1.0\" encoding=\"utf-8\"?>\r\n\
<!-- captured on site -->\r\n\
<!DOCTYPE ProbeMatch SYSTEM \"sadp.dtd\">\r\n\
<s:ProbeMatch xmlns:s=\"urn:sadp\">\r\n\
  <s:Types>Inquiry</s:Types>\r\n\
  <s:DeviceSN>  SN-XYZ  </s:DeviceSN>\r\n\
  <s:MAC>A4:14:37:11:22:33</s:MAC>\r\n\
  <s:IPv4Address>172.16.5.20</s:IPv4Address>\r\n\
  <s:VendorSpecific>hello</s:VendorSpecific>\r\n\
  <s:EmptyField></s:EmptyField>\r\n\
</s:ProbeMatch>\r\n";
        let d = decode_probe_match(xml.as_bytes()).unwrap().device;
        assert_eq!(d.sn.as_deref(), Some("SN-XYZ"));
        assert_eq!(d.ipv4.unwrap().to_string(), "172.16.5.20");
        assert_eq!(
            d.extra.get("vendorspecific").map(String::as_str),
            Some("hello")
        );
        assert!(!d.extra.contains_key("emptyfield"));
    }

    #[test]
    fn rejects_non_discovery_payloads() {
        assert!(matches!(decode_probe_match(b""), Err(DecodeError::Empty)));
        assert!(matches!(
            decode_probe_match(b"hello world"),
            Err(DecodeError::NotProbeMatch)
        ));
        let write_reply = br#"<ProbeMatch><Types>set</Types><DeviceSN>S</DeviceSN></ProbeMatch>"#;
        assert!(matches!(
            decode_probe_match(write_reply),
            Err(DecodeError::NotProbeMatch)
        ));
    }

    #[test]
    fn never_panics_on_arbitrary_bytes() {
        let samples: &[&[u8]] = &[
            b"\xff\xfe\x00\x00",
            b"<ProbeMatch><IPv4Address>999.1.1.1</IPv4Address></ProbeMatch>",
            b"<ProbeMatch><MAC>zz</MAC></ProbeMatch>",
            b"<ProbeMatch><DeviceSN>",
            b"<<<>>>",
            b"\0\0\0\0",
        ];
        for s in samples {
            let _ = decode_probe_match(s);
        }
    }

    #[test]
    fn detects_probe_match_tag_case_insensitively() {
        assert!(looks_like_probe_match(b"<?xml?><ProbeMatch>"));
        assert!(looks_like_probe_match(b"<probematch>"));
        assert!(looks_like_probe_match(b"<PROBEMATCH>"));
        assert!(!looks_like_probe_match(b"<Probe><Uuid>x</Uuid></Probe>"));
        assert!(!looks_like_probe_match(b"hello"));
    }

    #[test]
    fn rejects_payload_with_only_unknown_fields() {
        // 端口上的杂散报文可能被宽松解析成「未知字段」，绝不能变成幽灵设备。
        let xml = b"<ProbeMatch><Types>inquiry</Types><VendorOnly>42</VendorOnly></ProbeMatch>";
        assert!(matches!(
            decode_probe_match(xml),
            Err(DecodeError::NotProbeMatch)
        ));
    }

    #[test]
    fn zero_ipv4_is_treated_as_unset() {
        let xml = b"<ProbeMatch><MAC>a4:14:37:11:22:33</MAC><IPv4Address>0.0.0.0</IPv4Address><IPv4Gateway>0.0.0.0</IPv4Gateway></ProbeMatch>";
        let d = decode_probe_match(xml).expect("parse").device;
        assert_eq!(d.ipv4, None);
        assert_eq!(d.gateway, None);
        assert!(d.is_identifiable(), "MAC 仍然让它可识别");
    }

    #[test]
    fn accepts_link_local_only_ipv6() {
        // 只有链路本地地址也是有效身份：至少能显示、能复制。
        let xml = b"<ProbeMatch><IPv6Address>fe80::1%22</IPv6Address></ProbeMatch>";
        let d = decode_probe_match(xml).expect("parse").device;
        assert!(d.is_identifiable());
        assert_eq!(d.ipv6.len(), 1);
        assert_eq!(d.ipv6[0].scope_id, Some(22));
    }

    #[test]
    fn caps_oversized_values() {
        let big = "A".repeat(10_000);
        let xml = format!("<ProbeMatch><DeviceSN>{big}</DeviceSN></ProbeMatch>");
        let d = decode_probe_match(xml.as_bytes()).unwrap().device;
        assert_eq!(d.sn.unwrap().len(), MAX_VALUE_LEN);
    }
}
