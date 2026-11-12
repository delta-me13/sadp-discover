//! `Probe` 请求编码。
//!
//! 报文格式与官方 SADPTool 一致（UTF-8 XML，无命名空间）：
//!
//! ```xml
//! <?xml version="1.0" encoding="utf-8"?>
//! <Probe><Uuid>...</Uuid><Types>inquiry</Types></Probe>
//! ```
//!
//! `Uuid` 只是本次会话标识，设备不校验其唯一性，但部分固件会把它原样回显，
//! 因此每次扫描使用新的 v4 UUID，便于在抓包中区分探测轮次。

use uuid::Uuid;

/// 生成一次探测用的 UUID 字符串。
#[must_use]
pub fn new_probe_uuid() -> String {
    Uuid::new_v4().to_string()
}

/// 编码 `Probe` 报文。`uuid` 中的非安全字符会被过滤，避免 XML 注入。
#[must_use]
pub fn encode_probe(uuid: &str) -> Vec<u8> {
    let safe: String = uuid
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
        .take(64)
        .collect();
    let safe = if safe.is_empty() {
        new_probe_uuid()
    } else {
        safe
    };
    format!(
        "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<Probe>\n<Uuid>{safe}</Uuid>\n<Types>inquiry</Types>\n</Probe>\n"
    )
    .into_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn probe_is_well_formed_and_contains_inquiry() {
        let uuid = new_probe_uuid();
        let bytes = encode_probe(&uuid);
        let text = std::str::from_utf8(&bytes).unwrap();
        assert!(text.starts_with("<?xml version=\"1.0\" encoding=\"utf-8\"?>"));
        assert!(text.contains(&format!("<Uuid>{uuid}</Uuid>")));
        assert!(text.contains("<Types>inquiry</Types>"));
        assert!(text.trim_end().ends_with("</Probe>"));
    }

    #[test]
    fn probe_filters_xml_metacharacters() {
        let bytes = encode_probe("</Uuid><Evil/>");
        let text = std::str::from_utf8(&bytes).unwrap();
        assert!(!text.contains("<Evil"));
        assert!(text.contains("<Uuid>UuidEvil</Uuid>"));
    }

    #[test]
    fn empty_uuid_falls_back_to_generated_one() {
        let text = String::from_utf8(encode_probe("")).unwrap();
        let inner = text
            .split("<Uuid>")
            .nth(1)
            .unwrap()
            .split("</Uuid>")
            .next()
            .unwrap();
        assert_eq!(inner.len(), 36);
    }
}
