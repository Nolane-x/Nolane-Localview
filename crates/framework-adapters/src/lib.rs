#![forbid(unsafe_code)]

use serde::{Deserialize,Serialize};

#[derive(Debug,Clone,Serialize,Deserialize,PartialEq,Eq)] pub struct FrameworkSignals{pub framework:String,pub dev_server:Option<String>,pub component_runtime:Option<String>,pub confidence:u8,pub evidence:Vec<String>}

pub fn detect_from_html(html:&str)->Vec<FrameworkSignals>{
    let h=html.to_ascii_lowercase();let mut out=Vec::new();
    let specs=[("Vite",vec!["/@vite/client","vite/client"]),("Next.js",vec!["/_next/","__next_data__"]),("Nuxt",vec!["/_nuxt/","__nuxt"]),("Astro",vec!["astro-island","astro-slot"]),("SvelteKit",vec!["__sveltekit","data-sveltekit"]),("Angular",vec!["ng-version","ng-app"]),("Storybook",vec!["storybook-root","__storybook"]),("Remix",vec!["__remixcontext","__remixmanifest"])];
    for (name,markers) in specs{let matched=markers.iter().filter(|m|h.contains(**m)).map(|m|(*m).to_string()).collect::<Vec<_>>();if !matched.is_empty(){out.push(FrameworkSignals{framework:name.into(),dev_server:(name=="Vite").then(||"Vite".into()),component_runtime:Some(name.into()),confidence:(82+matched.len()*8).min(100)as u8,evidence:matched});}}
    out.sort_by_key(|x|std::cmp::Reverse(x.confidence));out
}

pub fn detect_from_files(paths:&[String])->Vec<FrameworkSignals>{let joined=paths.join("\n").to_ascii_lowercase();let mut html=String::new();if joined.contains("vite.config"){html.push_str("/@vite/client ");}if joined.contains("next.config"){html.push_str("/_next/ ");}if joined.contains("nuxt.config"){html.push_str("/_nuxt/ ");}if joined.contains("astro.config"){html.push_str("astro-island ");}if joined.contains("svelte.config"){html.push_str("__sveltekit ");}detect_from_html(&html)}

#[cfg(test)]mod tests{use super::*;#[test]fn ranks_vite(){let x=detect_from_html("<script src='/@vite/client'></script>");assert_eq!(x[0].framework,"Vite");}}
