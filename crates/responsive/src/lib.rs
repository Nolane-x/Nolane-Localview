#![forbid(unsafe_code)]

use async_trait::async_trait;
use serde::{Deserialize,Serialize};

#[derive(Debug,Clone,Copy,Serialize,Deserialize,PartialEq,Eq,Hash)] pub struct Viewport{pub width:u32,pub height:u32}
pub const DEFAULT_VIEWPORTS:&[Viewport]=&[Viewport{width:320,height:568},Viewport{width:360,height:800},Viewport{width:375,height:812},Viewport{width:390,height:844},Viewport{width:430,height:932},Viewport{width:768,height:1024},Viewport{width:1024,height:768},Viewport{width:1280,height:720},Viewport{width:1440,height:900},Viewport{width:1920,height:1080}];
#[async_trait] pub trait LayoutProbe:Send+Sync{async fn fails_at(&self,width:u32)->bool;}

pub async fn discover_breakpoint<P:LayoutProbe>(probe:&P,known_good:u32,known_bad:u32,tolerance:u32)->Option<u32>{
    if known_good==known_bad{return None;} let(mut low,mut high)=if known_bad<known_good{(known_bad,known_good)}else{(known_good,known_bad)};let low_fails=probe.fails_at(low).await;let high_fails=probe.fails_at(high).await;if low_fails==high_fails{return None;}
    while high-low>tolerance.max(1){let mid=low+(high-low)/2;if probe.fails_at(mid).await==low_fails{low=mid}else{high=mid;}}
    Some(if low_fails{high}else{low})
}

pub fn adaptive_sweep(min:u32,max:u32,anchors:&[u32])->Vec<u32>{let mut widths=anchors.iter().copied().filter(|w|*w>=min&&*w<=max).collect::<Vec<_>>();widths.extend([min,max]);widths.sort_unstable();widths.dedup();let mut extra=Vec::new();for pair in widths.windows(2){if pair[1]-pair[0]>160{extra.push(pair[0]+(pair[1]-pair[0])/2);}}widths.extend(extra);widths.sort_unstable();widths.dedup();widths}

#[cfg(test)]mod tests{use super::*;struct P;#[async_trait]impl LayoutProbe for P{async fn fails_at(&self,w:u32)->bool{w<728}}#[tokio::test]async fn finds_transition(){let b=discover_breakpoint(&P,768,700,2).await.unwrap();assert!((727..=730).contains(&b));}}
