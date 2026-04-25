//! relay-rs v1 控制面协议（由 tonic-build 从 `proto/relay.proto` 生成）
//!
//! 该 crate 是 v1.0 的**单一权威配置模型**来源 —— master 与 node 两端
//! 共用本 crate 生成的类型，避免出现两份并行 schema。

#![allow(clippy::all)]

pub mod v1 {
    tonic::include_proto!("relay.v1");
}

pub use v1::*;

/// desired/actual envelope hash：master/node 必须两端一致。
///
/// 规则：按 segment.id 升序排序后，对每个 Segment 写入 `u32_be(len) || prost_encode(seg)`，
/// 最后追加 `u32_be(ca_bundle_version)`，取 sha256。
///
/// 依赖 prost 的确定编码：字段号升序、skip default、无 map/unknown。
pub fn envelope_hash(segments: &[v1::Segment], ca_bundle_version: u32) -> Vec<u8> {
    use prost::Message;
    use sha2::{Digest, Sha256};
    let mut sorted: Vec<&v1::Segment> = segments.iter().collect();
    sorted.sort_by(|a, b| a.id.cmp(&b.id));

    let mut hasher = Sha256::new();
    for s in sorted {
        let buf = s.encode_to_vec();
        hasher.update((buf.len() as u32).to_be_bytes());
        hasher.update(&buf);
    }
    hasher.update(ca_bundle_version.to_be_bytes());
    hasher.finalize().to_vec()
}
