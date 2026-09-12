#![forbid(unsafe_code)]
use std::collections::BTreeMap;
use serde::{Deserialize,Serialize};
#[derive(Debug,Clone,Serialize,Deserialize,PartialEq,Eq,PartialOrd,Ord)]pub enum ConsoleLevel{Debug,Info,Warning,Error}
#[derive(Debug,Clone,Serialize,Deserialize)]pub struct ConsoleEntry{pub level:ConsoleLevel,pub message:String,pub stack:Option<String>,pub source:Option<String>,pub action_ref:Option<String>}
#[derive(Debug,Clone,Serialize,Deserialize)]pub struct ConsoleGroup{pub fingerprint:String,pub level:ConsoleLevel,pub message:String,pub count:u32,pub first_source:Option<String>,pub related_action:Option<String>}
pub fn group(entries:&[ConsoleEntry])->Vec<ConsoleGroup>{let mut map:BTreeMap<String,ConsoleGroup>=BTreeMap::new();for e in entries{let fp=fingerprint(&e.message,e.source.as_deref());map.entry(fp.clone()).and_modify(|g|g.count+=1).or_insert(ConsoleGroup{fingerprint:fp,level:e.level.clone(),message:e.message.clone(),count:1,first_source:e.source.clone(),related_action:e.action_ref.clone()});}map.into_values().collect()}
fn fingerprint(message:&str,source:Option<&str>)->String{let normalized=message.split_whitespace().collect::<Vec<_>>().join(" ");format!("{}|{}",normalized,source.unwrap_or(""))}
#[cfg(test)]mod tests{use super::*;#[test]fn deduplicates(){let e=ConsoleEntry{level:ConsoleLevel::Warning,message:"React warning".into(),stack:None,source:Some("App.tsx".into()),action_ref:None};let g=group(&[e.clone(),e]);assert_eq!(g[0].count,2);}}
