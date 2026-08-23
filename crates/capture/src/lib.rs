#![forbid(unsafe_code)]

use localview_protocol::{ElementRef,Rect};
use serde::{Deserialize,Serialize};

#[derive(Debug,Clone,Serialize,Deserialize,PartialEq)]
#[serde(tag="kind",rename_all="snake_case")]
pub enum CaptureTarget { Viewport, FullPage, Element{reference:ElementRef}, Region{rect:Rect}, Responsive{viewports:Vec<(u32,u32)>} }
#[derive(Debug,Clone,Serialize,Deserialize,PartialEq)]
pub struct StableCapturePolicy { pub wait_dom_ready:bool,pub wait_fonts:bool,pub wait_images:bool,pub wait_hmr_settle:bool,pub wait_layout_stable:bool,pub network_quiet_ms:Option<u64>,pub freeze_animation:bool,pub freeze_transition:bool,pub mask_selectors:Vec<String>,pub timeout_ms:u64 }
impl Default for StableCapturePolicy { fn default()->Self{Self{wait_dom_ready:true,wait_fonts:true,wait_images:true,wait_hmr_settle:true,wait_layout_stable:true,network_quiet_ms:Some(250),freeze_animation:true,freeze_transition:true,mask_selectors:vec![],timeout_ms:5_000}} }
#[derive(Debug,Clone,Serialize,Deserialize,PartialEq)] pub enum CaptureStage { DomReady,FontsReady,ImagesReady,HmrSettled,LayoutStable,NetworkQuiet,AnimationsFrozen,Masked,Captured,Restored }
#[derive(Debug,Clone,Serialize,Deserialize,PartialEq)] pub struct CapturePlan { pub target:CaptureTarget,pub policy:StableCapturePolicy,pub stages:Vec<CaptureStage> }

pub fn build_plan(target:CaptureTarget,policy:StableCapturePolicy)->CapturePlan{let mut stages=Vec::new();if policy.wait_dom_ready{stages.push(CaptureStage::DomReady);}if policy.wait_fonts{stages.push(CaptureStage::FontsReady);}if policy.wait_images{stages.push(CaptureStage::ImagesReady);}if policy.wait_hmr_settle{stages.push(CaptureStage::HmrSettled);}if policy.wait_layout_stable{stages.push(CaptureStage::LayoutStable);}if policy.network_quiet_ms.is_some(){stages.push(CaptureStage::NetworkQuiet);}if policy.freeze_animation||policy.freeze_transition{stages.push(CaptureStage::AnimationsFrozen);}if !policy.mask_selectors.is_empty(){stages.push(CaptureStage::Masked);}stages.push(CaptureStage::Captured);stages.push(CaptureStage::Restored);CapturePlan{target,policy,stages}}
pub fn progressive_regions(element:&Rect,component:Option<&Rect>,section:Option<&Rect>,viewport:(u32,u32))->Vec<Rect>{let mut out=vec![expand(element,120.0,viewport)];if let Some(r)=component{out.push(clamp(r,viewport));}if let Some(r)=section{out.push(clamp(r,viewport));}out.push(Rect{x:0.0,y:0.0,width:viewport.0 as f64,height:viewport.1 as f64});out}
fn expand(r:&Rect,pad:f64,v:(u32,u32))->Rect{clamp(&Rect{x:r.x-pad,y:r.y-pad,width:r.width+pad*2.0,height:r.height+pad*2.0},v)}
fn clamp(r:&Rect,v:(u32,u32))->Rect{let x=r.x.max(0.0).min(v.0 as f64);let y=r.y.max(0.0).min(v.1 as f64);Rect{x,y,width:r.width.min(v.0 as f64-x).max(0.0),height:r.height.min(v.1 as f64-y).max(0.0)}}

#[cfg(test)]mod tests{use super::*;#[test]fn plan_restores_after_capture(){let p=build_plan(CaptureTarget::Viewport,Default::default());assert_eq!(p.stages.last(),Some(&CaptureStage::Restored));}}
