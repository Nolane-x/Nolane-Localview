#![forbid(unsafe_code)]
use std::collections::BTreeMap;
use serde::{Deserialize,Serialize};
#[derive(Debug,Clone,Serialize,Deserialize,Default)]pub struct DesignSamples{pub spacing:Vec<f64>,pub radius:Vec<f64>,pub font_sizes:Vec<f64>,pub control_heights:Vec<f64>}
#[derive(Debug,Clone,Serialize,Deserialize,Default)]pub struct DesignGrammar{pub spacing_scale:Vec<f64>,pub radius_scale:Vec<f64>,pub type_scale:Vec<f64>,pub control_heights:Vec<f64>,pub confidence:u8}
pub fn infer(s:&DesignSamples)->DesignGrammar{let spacing=cluster(&s.spacing,2.0);let radius=cluster(&s.radius,1.5);let type_scale=cluster(&s.font_sizes,1.0);let control_heights=cluster(&s.control_heights,2.0);let populated=[!spacing.is_empty(),!radius.is_empty(),!type_scale.is_empty(),!control_heights.is_empty()].into_iter().filter(|x|*x).count();DesignGrammar{spacing_scale:spacing,radius_scale:radius,type_scale,control_heights,confidence:(populated*22).min(88)as u8}}
pub fn drift(value:f64,scale:&[f64],tolerance:f64)->Option<f64>{scale.iter().map(|s|(value-s).abs()).min_by(|a,b|a.total_cmp(b)).filter(|d|*d>tolerance)}
fn cluster(values:&[f64],bucket:f64)->Vec<f64>{let mut counts:BTreeMap<i64,usize>=BTreeMap::new();for v in values.iter().copied().filter(|v|v.is_finite()&&*v>0.0){let key=(v/bucket).round()as i64;*counts.entry(key).or_default()+=1;}counts.into_iter().filter(|(_,n)|*n>=2).map(|(k,_)|k as f64*bucket).collect()}
#[cfg(test)]mod tests{use super::*;#[test]fn learns_repeated_spacing(){let g=infer(&DesignSamples{spacing:vec![8.0,8.2,16.0,16.1,24.0,24.2],..Default::default()});assert_eq!(g.spacing_scale,vec![8.0,16.0,24.0]);}}
