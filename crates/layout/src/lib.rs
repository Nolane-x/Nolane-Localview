#![forbid(unsafe_code)]

use localview_protocol::{ElementRef, Rect};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LayoutElement { pub reference: ElementRef, pub rect: Rect, pub parent: Option<ElementRef>, pub font_size: Option<f64>, pub padding: Option<[f64;4]>, pub z_index: Option<i32> }
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LayoutIssue { pub code:String, pub severity:Severity, pub confidence:f32, pub refs:Vec<ElementRef>, pub message:String, pub evidence:String }
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)] #[serde(rename_all="snake_case")] pub enum Severity { Info, Warning, Error }

pub fn audit(elements:&[LayoutElement], viewport:(f64,f64))->Vec<LayoutIssue>{
    let mut issues=Vec::new();
    for e in elements {
        if e.rect.x < -0.5 || e.rect.y < -0.5 || e.rect.x+e.rect.width > viewport.0+0.5 || e.rect.y+e.rect.height > viewport.1+0.5 {
            issues.push(LayoutIssue{code:"viewport_overflow".into(),severity:Severity::Error,confidence:1.0,refs:vec![e.reference.clone()],message:"Element extends outside viewport".into(),evidence:format!("rect={:?} viewport={}x{}",e.rect,viewport.0,viewport.1)});
        }
        if e.rect.width <= 0.0 || e.rect.height <= 0.0 { issues.push(LayoutIssue{code:"zero_area".into(),severity:Severity::Warning,confidence:1.0,refs:vec![e.reference.clone()],message:"Element has zero visual area".into(),evidence:format!("{}x{}",e.rect.width,e.rect.height)}); }
    }
    for i in 0..elements.len(){ for j in i+1..elements.len(){ let a=&elements[i];let b=&elements[j]; if a.parent==b.parent && meaningful_overlap(&a.rect,&b.rect)>0.35 {issues.push(LayoutIssue{code:"sibling_overlap".into(),severity:Severity::Warning,confidence:0.92,refs:vec![a.reference.clone(),b.reference.clone()],message:"Sibling elements overlap substantially".into(),evidence:format!("overlap_ratio={:.2}",meaningful_overlap(&a.rect,&b.rect))});}}}
    issues.extend(alignment_anomalies(elements)); issues
}

fn meaningful_overlap(a:&Rect,b:&Rect)->f64 {let x=(a.x+a.width).min(b.x+b.width)-a.x.max(b.x);let y=(a.y+a.height).min(b.y+b.height)-a.y.max(b.y);if x<=0.0||y<=0.0{return 0.0;} let intersection=x*y; intersection/(a.width*a.height).min(b.width*b.height).max(1.0)}
fn alignment_anomalies(elements:&[LayoutElement])->Vec<LayoutIssue>{
    if elements.len()<3{return vec![];} let mut xs=elements.iter().map(|e|e.rect.x).collect::<Vec<_>>(); xs.sort_by(|a,b|a.total_cmp(b));
    let median=xs[xs.len()/2]; elements.iter().filter(|e|(e.rect.x-median).abs()>3.0 && (e.rect.x-median).abs()<12.0).map(|e|LayoutIssue{code:"alignment_drift".into(),severity:Severity::Info,confidence:0.72,refs:vec![e.reference.clone()],message:"Possible repeated-edge alignment drift".into(),evidence:format!("x={:.1}, family≈{median:.1}",e.rect.x)}).collect()
}

pub fn infer_spacing_scale(values:&[f64])->Vec<f64>{ let mut rounded=values.iter().copied().filter(|v|*v>0.0).map(|v|(v/2.0).round()*2.0).collect::<Vec<_>>(); rounded.sort_by(|a,b|a.total_cmp(b)); rounded.dedup_by(|a,b|(*a-*b).abs()<1.0); rounded }

#[cfg(test)] mod tests {use super::*; #[test] fn catches_overflow(){let e=LayoutElement{reference:"@x".into(),rect:Rect{x:390.0,y:0.0,width:40.0,height:40.0},parent:None,font_size:None,padding:None,z_index:None};assert!(audit(&[e],(400.0,800.0)).iter().any(|x|x.code=="viewport_overflow"));}}
