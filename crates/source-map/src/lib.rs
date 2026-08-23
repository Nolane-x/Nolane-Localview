#![forbid(unsafe_code)]

use localview_protocol::SourceLocation;
use regex::Regex;
use serde::{Deserialize,Serialize};

#[derive(Debug,Clone,Serialize,Deserialize,PartialEq,Eq)] pub struct SourceHint{pub component:Option<String>,pub file:Option<String>,pub line:Option<u32>,pub confidence:u8,pub origin:String}

pub fn parse_stack_locations(stack:&str)->Vec<SourceLocation>{
    let re=Regex::new(r"(?m)(?:\(|\s|^)([^\s()]+\.(?:tsx?|jsx?|vue|svelte)):(\d+):(\d+)\)?").unwrap();
    re.captures_iter(stack).filter_map(|c|Some(SourceLocation{file:c.get(1)?.as_str().to_owned(),line:c.get(2)?.as_str().parse().ok()?,column:c.get(3).and_then(|m|m.as_str().parse().ok()),component:None})).collect()
}

pub fn rank_hints(component:Option<&str>,stack:&str,attributes:&[(String,String)])->Vec<SourceHint>{
    let mut hints=parse_stack_locations(stack).into_iter().map(|s|SourceHint{component:component.map(str::to_owned),file:Some(s.file),line:Some(s.line),confidence:90,origin:"stack".into()}).collect::<Vec<_>>();
    for (k,v) in attributes{if k=="data-source"||k=="data-component-source"{let(mut file,mut line)=(v.as_str(),None);if let Some(i)=v.rfind(':'){if let Ok(n)=v[i+1..].parse(){file=&v[..i];line=Some(n);}}hints.push(SourceHint{component:component.map(str::to_owned),file:Some(file.to_owned()),line,confidence:100,origin:k.clone()});}}
    hints.sort_by_key(|h|std::cmp::Reverse(h.confidence));hints
}

#[cfg(test)]mod tests{use super::*;#[test]fn parses_vite_stack(){let x=parse_stack_locations("at save (src/components/Button.tsx:84:12)");assert_eq!(x[0].line,84);}}
