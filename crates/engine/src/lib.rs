#![forbid(unsafe_code)]
use serde::{Deserialize,Serialize};

#[derive(Debug,Clone,Copy,Serialize,Deserialize,PartialEq,Eq,PartialOrd,Ord)] pub enum EngineTier { Static=0,Lightweight=1,NativeWebView=2,Chromium=3 }
#[derive(Debug,Clone,Serialize,Deserialize,Default)] pub struct EngineNeeds { pub source_only:bool,pub javascript:bool,pub interaction:bool,pub screenshot:bool,pub exact_platform_render:bool,pub chrome_compatibility:bool,pub devtools_trace:bool,pub advanced_emulation:bool }
#[derive(Debug,Clone,Serialize,Deserialize)] pub struct EngineDecision { pub tier:EngineTier,pub reasons:Vec<String> }
pub fn choose_engine(n:&EngineNeeds)->EngineDecision{let mut reasons=Vec::new();let tier=if n.chrome_compatibility||n.devtools_trace||n.advanced_emulation{reasons.push("browser-specific capability requested".into());EngineTier::Chromium}else if n.screenshot||n.exact_platform_render{reasons.push("human-visible native rendering required".into());EngineTier::NativeWebView}else if n.javascript||n.interaction{reasons.push("semantic runtime execution required".into());EngineTier::Lightweight}else{reasons.push("static/source inspection is sufficient".into());EngineTier::Static};EngineDecision{tier,reasons}}
#[cfg(test)]mod tests{use super::*;#[test]fn chromium_is_not_default(){assert_eq!(choose_engine(&EngineNeeds{javascript:true,..Default::default()}).tier,EngineTier::Lightweight);}}
