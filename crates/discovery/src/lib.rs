#![forbid(unsafe_code)]

use std::{collections::{HashMap, HashSet}, net::IpAddr, process::Stdio, time::Duration};
use anyhow::{Context, Result};
use async_trait::async_trait;
use futures::{stream, StreamExt};
use localview_protocol::{Classification, DiscoveredServer, Endpoint, ListenerCandidate, ServerKind};
use regex::Regex;
use reqwest::Client;
use tokio::process::Command;

#[async_trait]
pub trait ListenerSource: Send + Sync {
    async fn listeners(&self) -> Result<Vec<ListenerCandidate>>;
}

#[derive(Debug, Default)]
pub struct CommandListenerSource;

#[async_trait]
impl ListenerSource for CommandListenerSource {
    async fn listeners(&self) -> Result<Vec<ListenerCandidate>> {
        #[cfg(target_os = "windows")]
        let (program, args) = ("netstat", vec!["-ano", "-p", "tcp"]);
        #[cfg(target_os = "linux")]
        let (program, args) = ("ss", vec!["-ltnpH"]);
        #[cfg(target_os = "macos")]
        let (program, args) = ("lsof", vec!["-nP", "-iTCP", "-sTCP:LISTEN"]);
        #[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
        return Ok(Vec::new());

        let output = Command::new(program).args(args).stdout(Stdio::piped()).stderr(Stdio::null()).output().await
            .with_context(|| format!("failed to execute listener source {program}"))?;
        let text = String::from_utf8_lossy(&output.stdout);
        #[cfg(target_os = "windows")]
        return Ok(parse_windows_netstat(&text));
        #[cfg(target_os = "linux")]
        return Ok(parse_linux_ss(&text));
        #[cfg(target_os = "macos")]
        return Ok(parse_macos_lsof(&text));
    }
}

pub struct HttpClassifier { client: Client }

impl HttpClassifier {
    pub fn new(timeout: Duration) -> Result<Self> {
        Ok(Self { client: Client::builder().timeout(timeout).redirect(reqwest::redirect::Policy::limited(2)).build()? })
    }

    pub async fn classify(&self, candidate: &ListenerCandidate) -> Result<Classification> {
        let url = candidate.endpoint.url()?;
        let response = self.client.get(url).header("user-agent", "LocalView/0.2 discovery").send().await?;
        let status = response.status();
        let headers = response.headers().clone();
        let body = response.text().await.unwrap_or_default();
        let sample = body.chars().take(256_000).collect::<String>();
        Ok(classify_response(status.as_u16(), &headers, &sample))
    }
}

pub struct DiscoveryEngine<S> { source: S, classifier: HttpClassifier, concurrency: usize }

impl<S: ListenerSource> DiscoveryEngine<S> {
    pub fn new(source: S, timeout: Duration, concurrency: usize) -> Result<Self> {
        Ok(Self { source, classifier: HttpClassifier::new(timeout)?, concurrency: concurrency.max(1) })
    }

    pub async fn scan(&self) -> Result<Vec<DiscoveredServer>> {
        let listeners = self.source.listeners().await?;
        let mut seen = HashSet::new();
        let candidates = listeners.into_iter().filter(|c| is_loopback_host(&c.endpoint.host)).filter(|c| seen.insert((c.endpoint.host.clone(), c.endpoint.port))).collect::<Vec<_>>();
        let results = stream::iter(candidates.into_iter().map(|candidate| async move {
            self.classifier.classify(&candidate).await.ok().map(|classification| DiscoveredServer { candidate, classification })
        })).buffer_unordered(self.concurrency).filter_map(|x| async move { x }).collect().await;
        Ok(results)
    }
}

fn is_loopback_host(host: &str) -> bool {
    host == "localhost" || host == "::1" || host.parse::<IpAddr>().map(|ip| ip.is_loopback()).unwrap_or(false)
}

pub fn classify_response(status: u16, headers: &http::HeaderMap, body: &str) -> Classification {
    let lower = body.to_ascii_lowercase();
    let content_type = headers.get(http::header::CONTENT_TYPE).and_then(|v| v.to_str().ok()).unwrap_or("").to_ascii_lowercase();
    let html = content_type.contains("text/html") || lower.contains("<html") || lower.contains("<!doctype html");
    let mut evidence = smallvec::SmallVec::new();
    if html { evidence.push("html-document".to_string()); }
    if status < 500 { evidence.push(format!("http-{status}")); }
    let markers: [(&str, &str); 10] = [
        ("/@vite/client", "Vite"), ("__next", "Next.js"), ("/_next/", "Next.js"),
        ("__nuxt", "Nuxt"), ("/_nuxt/", "Nuxt"), ("svelte", "Svelte/SvelteKit"),
        ("astro-island", "Astro"), ("ng-version", "Angular"), ("webpack", "Webpack"),
        ("storybook", "Storybook"),
    ];
    let framework = markers.iter().find(|(needle, _)| lower.contains(needle)).map(|(_, name)| (*name).to_string());
    if let Some(name) = &framework { evidence.push(format!("framework:{name}")); }
    let hmr = lower.contains("/@vite/client") || lower.contains("webpackhotupdat") || lower.contains("hot-update") || lower.contains("__vite__");
    if hmr { evidence.push("hmr-marker".to_string()); }
    let api_like = !html && (content_type.contains("json") || lower.trim_start().starts_with('{') || lower.trim_start().starts_with('['));
    let kind = if framework.as_deref() == Some("Storybook") { ServerKind::Storybook }
        else if html && (framework.is_some() || hmr) { ServerKind::FrontendDevServer }
        else if html { ServerKind::StaticSite }
        else if api_like { ServerKind::ApiServer }
        else { ServerKind::UnknownHttp };
    let confidence = match kind { ServerKind::FrontendDevServer | ServerKind::Storybook => 0.98, ServerKind::StaticSite => 0.78, ServerKind::ApiServer => 0.88, ServerKind::UnknownHttp => 0.45 };
    Classification { kind, confidence, framework, title: extract_title(body), hmr_detected: hmr, evidence }
}

fn extract_title(body: &str) -> Option<String> {
    let re = Regex::new(r"(?is)<title[^>]*>(.*?)</title>").expect("static regex");
    re.captures(body).and_then(|c| c.get(1)).map(|m| m.as_str().split_whitespace().collect::<Vec<_>>().join(" ")).filter(|s| !s.is_empty())
}

pub fn parse_windows_netstat(input: &str) -> Vec<ListenerCandidate> {
    input.lines().filter_map(|line| {
        let parts = line.split_whitespace().collect::<Vec<_>>();
        if parts.len() < 5 || !parts[0].eq_ignore_ascii_case("TCP") || !parts[3].eq_ignore_ascii_case("LISTENING") { return None; }
        let (host, port) = split_addr(parts[1])?;
        Some(candidate(host, port, parts[4].parse().ok()))
    }).collect()
}

pub fn parse_linux_ss(input: &str) -> Vec<ListenerCandidate> {
    let pid_re = Regex::new(r"pid=(\d+)").expect("static regex");
    input.lines().filter_map(|line| {
        let parts = line.split_whitespace().collect::<Vec<_>>();
        if parts.len() < 4 { return None; }
        let addr = parts.iter().find(|p| p.rsplit(':').next().and_then(|x| x.parse::<u16>().ok()).is_some())?;
        let (host, port) = split_addr(addr)?;
        let pid = pid_re.captures(line).and_then(|c| c.get(1)).and_then(|m| m.as_str().parse().ok());
        Some(candidate(host, port, pid))
    }).collect()
}

pub fn parse_macos_lsof(input: &str) -> Vec<ListenerCandidate> {
    input.lines().skip(1).filter_map(|line| {
        let parts = line.split_whitespace().collect::<Vec<_>>();
        if parts.len() < 9 || !line.contains("(LISTEN)") { return None; }
        let addr = parts.iter().find(|p| p.contains(':') && !p.starts_with("TCP"))?;
        let (host, port) = split_addr(addr.trim_end_matches("(LISTEN)"))?;
        Some(ListenerCandidate { endpoint: Endpoint { host, port, scheme:"http".into() }, pid:parts.get(1).and_then(|x| x.parse().ok()), process_name:parts.first().map(|x| (*x).to_string()), command:None, cwd:None })
    }).collect()
}

fn split_addr(raw: &str) -> Option<(String, u16)> {
    let raw = raw.trim().trim_matches('[').trim_matches(']');
    let idx = raw.rfind(':')?;
    let host = raw[..idx].trim_matches('[').trim_matches(']').replace('*', "127.0.0.1");
    let port = raw[idx + 1..].parse().ok()?;
    Some((if host == "0.0.0.0" || host == "::" { "127.0.0.1".into() } else { host }, port))
}

fn candidate(host: String, port: u16, pid: Option<u32>) -> ListenerCandidate {
    ListenerCandidate { endpoint: Endpoint { host, port, scheme:"http".into() }, pid, process_name:None, command:None, cwd:None }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parses_windows_listener() {
        let rows = parse_windows_netstat("  TCP    127.0.0.1:5173   0.0.0.0:0   LISTENING   4242\n");
        assert_eq!(rows[0].endpoint.port, 5173); assert_eq!(rows[0].pid, Some(4242));
    }
    #[test]
    fn detects_vite_frontend() {
        let mut headers = http::HeaderMap::new(); headers.insert(http::header::CONTENT_TYPE, "text/html".parse().unwrap());
        let c = classify_response(200, &headers, "<html><title>App</title><script type=module src='/@vite/client'></script></html>");
        assert_eq!(c.kind, ServerKind::FrontendDevServer); assert_eq!(c.framework.as_deref(), Some("Vite")); assert!(c.hmr_detected);
    }
    #[test]
    fn detects_json_api() {
        let mut headers = http::HeaderMap::new(); headers.insert(http::header::CONTENT_TYPE, "application/json".parse().unwrap());
        assert_eq!(classify_response(200, &headers, "{\"ok\":true}").kind, ServerKind::ApiServer);
    }
}
