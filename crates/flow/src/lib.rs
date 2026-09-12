#![forbid(unsafe_code)]
use std::collections::{BTreeMap,VecDeque};
use serde::{Deserialize,Serialize};
#[derive(Debug,Clone,Serialize,Deserialize,PartialEq,Eq)]#[serde(tag="action",rename_all="snake_case")]pub enum Step{Navigate{url:String},Click{reference:String},Type{reference:String,text:String},Key{key:String},WaitRoute{route:String},AssertVisible{reference:String}}
#[derive(Debug,Clone,Serialize,Deserialize)]pub struct Flow{pub name:String,pub steps:Vec<Step>}
#[derive(Debug,Clone,Serialize,Deserialize,Default)]pub struct InteractionGraph{pub edges:BTreeMap<String,Vec<Transition>>}
#[derive(Debug,Clone,Serialize,Deserialize)]pub struct Transition{pub action:String,pub target:String,pub resulting_state:String}
impl InteractionGraph{pub fn record(&mut self,state:&str,transition:Transition){let e=self.edges.entry(state.into()).or_default();if !e.iter().any(|x|x.action==transition.action&&x.resulting_state==transition.resulting_state){e.push(transition);}}pub fn shortest_path(&self,from:&str,to:&str)->Option<Vec<Transition>>{let mut q=VecDeque::from([(from.to_string(),Vec::new())]);let mut seen=std::collections::HashSet::new();while let Some((state,path))=q.pop_front(){if state==to{return Some(path);}if !seen.insert(state.clone()){continue;}for edge in self.edges.get(&state).into_iter().flatten(){let mut next=path.clone();next.push(edge.clone());q.push_back((edge.resulting_state.clone(),next));}}None}}
#[cfg(test)]mod tests{use super::*;#[test]fn finds_flow(){let mut g=InteractionGraph::default();g.record("/",Transition{action:"login".into(),target:"@login".into(),resulting_state:"/login".into()});assert_eq!(g.shortest_path("/","/login").unwrap().len(),1);}}
