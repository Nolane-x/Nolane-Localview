#![forbid(unsafe_code)]

use localview_protocol::{DetailLevel,PageSnapshot,StateDiff,TokenBudget};
use serde::Serialize;

pub fn approximate_tokens(text:&str)->usize{(text.chars().count()+3)/4}
pub fn serialize_with_budget<T:Serialize>(value:&T,budget:&TokenBudget)->serde_json::Value{
    let full=serde_json::to_value(value).unwrap_or(serde_json::Value::Null);let serialized=serde_json::to_string(&full).unwrap_or_default();if approximate_tokens(&serialized)<=budget.max_tokens{return full;}
    match budget.detail {DetailLevel::Deep=>full,DetailLevel::Normal=>trim_json(full,budget.max_tokens*4),DetailLevel::Minimal=>summary_json(full)}
}
fn summary_json(value:serde_json::Value)->serde_json::Value{match value{serde_json::Value::Object(map)=>serde_json::Value::Object(map.into_iter().filter(|(k,_)|matches!(k.as_str(),"version"|"route"|"changed_refs"|"removed_refs"|"console_delta"|"network_delta"|"layout_changes")).collect()),other=>other}}
fn trim_json(mut value:serde_json::Value,max_chars:usize)->serde_json::Value{if let serde_json::Value::Object(map)=&mut value{for v in map.values_mut(){if let serde_json::Value::Array(a)=v{if a.len()>24{a.truncate(24);}}}}let mut text=serde_json::to_string(&value).unwrap_or_default();if text.len()>max_chars{text.truncate(max_chars.saturating_sub(40));serde_json::json!({"truncated":true,"preview":text})}else{value}}

#[cfg(test)]mod tests{use super::*;#[test]fn token_estimate_is_bounded(){assert_eq!(approximate_tokens("12345678"),2);}}
