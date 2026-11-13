//! SADP 协议、扫描引擎与国际化。
//!
//! 依赖方向：`sadp-core` → `sadp-net`；本 crate **不允许** 依赖任何终端或 GUI 库，
//! 以保证一阶段的 TUI 与三阶段的 GUI 复用同一套逻辑（ADR-004）。

rust_i18n::i18n!("locales", fallback = "en-US");

pub mod i18n;
pub mod proto;
pub mod scan;
pub mod system;
pub mod view;

pub use i18n::{Lang, tr, tr_args};
pub use proto::{
    Device, DeviceKey, Ipv6Endpoint, MacAddr, ProbeMatch, SeenVia, decode_probe_match, encode_probe,
};
pub use sadp_net::{Datagram, Family, InterfaceInfo, NetError, Transport};
pub use scan::{
    ConfigError, CoreError, DeviceStore, ScanConfig, ScanEvent, Scanner, Session, Upsert,
    run_blocking,
};

/// `--json` 输出的 schema 标识。字段只增不删，删除走 v2（ADR-005）。
pub const JSON_SCHEMA: &str = "sadp.discover/v1";

/// 语义版本号。
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
