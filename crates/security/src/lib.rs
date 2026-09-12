#![forbid(unsafe_code)]

use std::collections::HashSet;
use localview_protocol::Capability;
use regex::Regex;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PermissionPolicy { allowed: HashSet<Capability> }
impl Default for PermissionPolicy { fn default()->Self { Self { allowed:[Capability::Observe].into_iter().collect() } } }
impl PermissionPolicy { pub fn new(values: impl IntoIterator<Item=Capability>)->Self{Self{allowed:values.into_iter().collect()}} pub fn allows(&self,c:Capability)->bool{self.allowed.contains(&c)} }

#[derive(Clone)]
pub struct SecretRedactor { patterns: Vec<Regex> }
impl Default for SecretRedactor { fn default()->Self { Self { patterns: vec![
    Regex::new(r"(?i)(authorization\s*[:=]\s*(?:bearer\s+)?)[^\s,;]+" ).unwrap(),
    Regex::new(r"(?i)((?:api[_-]?key|token|password|secret)\s*[:=]\s*)[^\s,;]+" ).unwrap(),
    Regex::new(r"(?i)(cookie\s*[:=]\s*)[^\r\n]+" ).unwrap(),
] } } }
impl SecretRedactor { pub fn redact(&self, input:&str)->String{ self.patterns.iter().fold(input.to_owned(),|text,re|re.replace_all(&text,"${1}[REDACTED]").into_owned()) } }

pub fn generate_control_token() -> String { format!("lv_{}", Uuid::new_v4().simple()) }

#[cfg(test)] mod tests { use super::*; #[test] fn redacts_bearer(){ let r=SecretRedactor::default(); assert_eq!(r.redact("Authorization: Bearer abc123"),"Authorization: Bearer [REDACTED]"); } }
