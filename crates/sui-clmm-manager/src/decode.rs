//! Shared GraphQL JSON helpers for Move object fields.

use serde_json::Value;

pub fn json_u64(v: Option<&Value>) -> u64 {
    match v {
        Some(Value::Number(n)) => n.as_u64().unwrap_or(0),
        Some(Value::String(s)) => s.parse().unwrap_or(0),
        _ => 0,
    }
}

pub fn json_u128(v: Option<&Value>) -> u128 {
    match v {
        Some(Value::Number(n)) => n.as_u64().map(u128::from).unwrap_or(0),
        Some(Value::String(s)) => s.parse().unwrap_or(0),
        _ => 0,
    }
}

pub fn json_str(v: Option<&Value>) -> String {
    v.and_then(|x| x.as_str()).unwrap_or("").to_string()
}

pub fn json_bool(v: Option<&Value>) -> bool {
    v.and_then(|x| x.as_bool()).unwrap_or(false)
}

pub fn i32_bits(v: Option<&Value>) -> i32 {
    let bits = match v {
        Some(obj) => json_u64(obj.get("bits")) as u32,
        None => 0,
    };
    crate::pool::i32_from_bits(bits)
}

/// Ensure coin type has a `0x` prefix on the address segment.
pub fn normalize_coin_type(t: &str) -> String {
    let t = t.trim();
    if t.starts_with("0x") {
        return t.to_string();
    }
    if let Some((addr, rest)) = t.split_once("::") {
        format!("0x{addr}::{rest}")
    } else {
        format!("0x{t}")
    }
}

pub fn contents_json(node: &Value) -> Option<&Value> {
    node.pointer("/asMoveObject/contents/json")
        .or_else(|| node.pointer("/contents/json"))
        .or_else(|| node.get("json"))
}

pub fn contents_type(node: &Value) -> String {
    node.pointer("/asMoveObject/contents/type/repr")
        .or_else(|| node.pointer("/contents/type/repr"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string()
}
