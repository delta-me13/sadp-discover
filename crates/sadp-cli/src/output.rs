//! 输出层：JSON（冻结 schema）、CSV、纯文本表格、quiet。
//!
//! JSON 的字段名与顺序是**对外契约**（ADR-005），不随语言变化；
//! 只有人读的提示语走 i18n。

use std::io::{self, BufWriter, Write};
use std::path::Path;
use std::time::Duration;

use serde::Serialize;
use unicode_width::UnicodeWidthStr;

use sadp_core::i18n::{tr, tr_args};
use sadp_core::{Device, Family, JSON_SCHEMA, Session};

/// 输出所需的扫描元信息。
#[derive(Clone, Debug)]
pub struct Meta {
    pub port: u16,
    pub families: Vec<Family>,
    pub elapsed: Duration,
    pub rounds: u64,
}

impl Meta {
    #[must_use]
    pub fn from_session(session: &Session, port: u16, families: &[Family]) -> Self {
        Self {
            port,
            families: families.to_vec(),
            elapsed: session.elapsed(),
            rounds: session.round,
        }
    }
}

#[derive(Debug, Serialize)]
struct JsonIpv6 {
    address: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    scope_id: Option<u32>,
    global: bool,
}

#[derive(Debug, Serialize)]
struct JsonDevice {
    #[serde(skip_serializing_if = "Option::is_none")]
    sn: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    mac: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    ipv4: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    netmask: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    gateway: Option<String>,
    ipv6: Vec<JsonIpv6>,
    #[serde(skip_serializing_if = "Option::is_none")]
    ipv6_masklen: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    dhcp: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    firmware: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    sdk_port: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    http_port: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    activated: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    reset_ability: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    analog_channels: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    digital_channels: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    support: Option<String>,
    seen_via: String,
    seen_on: Vec<String>,
    first_seen_ago_ms: u64,
    last_seen_ago_ms: u64,
    #[serde(skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    extra: std::collections::BTreeMap<String, String>,
}

impl JsonDevice {
    fn from_device(d: &Device) -> Self {
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

#[derive(Debug, Serialize)]
struct JsonRoot {
    schema: &'static str,
    tool: &'static str,
    version: &'static str,
    generated_at: String,
    elapsed_ms: u64,
    port: u16,
    families: Vec<&'static str>,
    rounds: u64,
    count: usize,
    devices: Vec<JsonDevice>,
}

#[must_use]
pub fn generated_at() -> String {
    time::OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_else(|_| "1970-01-01T00:00:00Z".to_string())
}

pub fn write_json<W: Write>(w: &mut W, session: &Session, meta: &Meta) -> io::Result<()> {
    let root = JsonRoot {
        schema: JSON_SCHEMA,
        tool: "sadp-discover",
        version: sadp_core::VERSION,
        generated_at: generated_at(),
        elapsed_ms: meta.elapsed.as_millis() as u64,
        port: meta.port,
        families: meta.families.iter().map(|f| f.as_str()).collect(),
        rounds: meta.rounds,
        count: session.devices().len(),
        devices: session
            .devices()
            .iter()
            .map(JsonDevice::from_device)
            .collect(),
    };
    serde_json::to_writer_pretty(&mut *w, &root)?;
    writeln!(w)
}

#[derive(Debug, Serialize)]
struct CsvRow {
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
    fn from_device(d: &Device) -> Self {
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

pub fn write_csv<W: Write>(w: W, session: &Session) -> io::Result<()> {
    let mut writer = csv::WriterBuilder::new().from_writer(w);
    for device in session.devices() {
        writer
            .serialize(CsvRow::from_device(device))
            .map_err(|e| io::Error::other(e.to_string()))?;
    }
    writer.flush()
}

/// 纯文本表格。`width` 是终端列数（非 TTY 时用 120）。
pub fn write_table<W: Write>(
    w: &mut W,
    session: &Session,
    meta: &Meta,
    width: usize,
) -> io::Result<()> {
    if session.devices().is_empty() {
        writeln!(w, "{}", tr("out.no_devices"))?;
        return Ok(());
    }

    let compact = width < 90;
    let mut headers = vec![tr("tui.col.ip"), tr("tui.col.mac"), tr("tui.col.model")];
    if !compact {
        headers.push(tr("tui.col.sn"));
    }
    headers.push(tr("tui.col.via"));
    headers.push(tr("tui.col.activated"));

    let widths = column_widths(width, compact);
    let header_line: Vec<String> = headers
        .iter()
        .zip(&widths)
        .map(|(h, &w)| pad_display(h, w))
        .collect();
    writeln!(w, "{}", header_line.join("  ").trim_end())?;
    let header_width = UnicodeWidthStr::width(header_line.join("  ").trim_end());
    writeln!(w, "{}", dashes(header_width))?;

    for device in session.devices() {
        let mut cells = vec![
            device.primary_ip(),
            device.mac.map(|m| m.to_string()).unwrap_or_default(),
            device.display_model().unwrap_or_default().to_string(),
        ];
        if !compact {
            cells.push(device.sn.clone().unwrap_or_default());
        }
        cells.push(device.seen_via.as_str().to_string());
        cells.push(match device.activated {
            Some(true) => "✓".to_string(),
            Some(false) => "✗".to_string(),
            None => "-".to_string(),
        });

        let line: Vec<String> = cells
            .iter()
            .zip(&widths)
            .map(|(c, &w)| pad_display(c, w))
            .collect();
        writeln!(w, "{}", line.join("  ").trim_end())?;
    }

    writeln!(w)?;
    writeln!(
        w,
        "{}",
        tr_args(
            "out.summary",
            &[("count", &session.devices().len().to_string())]
        )
    )?;
    writeln!(
        w,
        "{}",
        tr_args(
            "out.elapsed",
            &[("elapsed", &format_duration(meta.elapsed))]
        )
    )?;
    if !session.errors.is_empty() {
        writeln!(
            w,
            "{}",
            tr_args(
                "out.errors",
                &[("count", &session.errors.len().to_string())]
            )
        )?;
    }
    Ok(())
}

pub fn write_quiet<W: Write>(w: &mut W, session: &Session) -> io::Result<()> {
    for device in session.devices() {
        writeln!(w, "{}", device.primary_ip())?;
    }
    Ok(())
}

pub fn export_json(path: &Path, session: &Session, meta: &Meta) -> io::Result<()> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent)?;
    }
    let file = std::fs::File::create(path)?;
    let mut writer = BufWriter::new(file);
    write_json(&mut writer, session, meta)?;
    writer.flush()
}

pub fn export_csv(path: &Path, session: &Session) -> io::Result<()> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent)?;
    }
    let file = std::fs::File::create(path)?;
    write_csv(BufWriter::new(file), session)
}

fn column_widths(width: usize, compact: bool) -> Vec<usize> {
    // Via 列固定 5（"both" 是 4），Act 列固定 4（"激活" 是 4，不能再窄）。
    if compact {
        // IP, MAC, Model, Via, Act
        let fixed = 25 + 17 + 5 + 4;
        let model = width.saturating_sub(fixed + 8).clamp(8, 28);
        vec![25, 17, model, 5, 4]
    } else {
        // IP, MAC, Model, SN, Via, Act
        let fixed = 25 + 17 + 5 + 4;
        let remaining = width.saturating_sub(fixed + 10);
        let model = (remaining * 2 / 5).clamp(12, 24);
        let sn = remaining.saturating_sub(model).clamp(10, 40);
        vec![25, 17, model, sn, 5, 4]
    }
}

/// 按终端显示宽度截断（中文/emoji 占 2 列），超出部分用 `…` 表示。
fn fit(text: &str, width: usize) -> String {
    if UnicodeWidthStr::width(text) <= width {
        return text.to_string();
    }
    if width == 0 {
        return String::new();
    }
    let mut out = String::new();
    let mut used = 0usize;
    for ch in text.chars() {
        let w = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0);
        if used + w > width.saturating_sub(1) {
            break;
        }
        out.push(ch);
        used += w;
    }
    out.push('…');
    out
}

/// 按显示宽度补空格（对外暴露，供其他表格输出复用）。
#[must_use]
pub fn pad_display(text: &str, width: usize) -> String {
    let fitted = fit(text, width);
    let used = UnicodeWidthStr::width(fitted.as_str());
    let mut out = fitted;
    out.push_str(&" ".repeat(width.saturating_sub(used)));
    out
}

fn dashes(n: usize) -> String {
    "-".repeat(n)
}

#[must_use]
pub fn format_duration(d: Duration) -> String {
    let secs = d.as_secs_f64();
    if secs < 1.0 {
        format!("{} ms", d.as_millis())
    } else {
        format!("{secs:.1} s")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fitting_respects_cjk_width() {
        let s = "球机-东门";
        assert_eq!(UnicodeWidthStr::width(s), 9);
        let fitted = fit(s, 6);
        assert!(UnicodeWidthStr::width(fitted.as_str()) <= 6);
        assert!(fitted.ends_with('…'));
    }

    #[test]
    fn padding_aligns_ascii_and_cjk() {
        assert_eq!(UnicodeWidthStr::width(pad_display("abc", 6).as_str()), 6);
        assert_eq!(UnicodeWidthStr::width(pad_display("中文", 6).as_str()), 6);
    }

    #[test]
    fn duration_formatting() {
        assert_eq!(format_duration(Duration::from_millis(250)), "250 ms");
        assert_eq!(format_duration(Duration::from_millis(1500)), "1.5 s");
    }
}
