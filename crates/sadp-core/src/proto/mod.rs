//! SADP 协议：模型、编码、解码。

pub mod encode;
pub mod model;
pub mod parse;

pub use encode::{encode_probe, new_probe_uuid};
pub use model::{Device, DeviceKey, Ipv6Endpoint, MacAddr, SeenVia};
pub use parse::{DecodeError, ProbeMatch, decode_probe_match, looks_like_probe_match};
