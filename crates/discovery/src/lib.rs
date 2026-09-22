#![forbid(unsafe_code)]

use std::{
    collections::HashSet,
    net::IpAddr,
    path::PathBuf,
    process::Stdio,
    time::Duration,
};
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
        let Some((program, args)) = trusted_listener_command() else {
            return Ok(Vec::new());
        };

        let output = Command::new(&program)
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .env_remove("LD_PRELOAD")
            .env_remove("LD_LIBRARY_PATH")
            .env_remove("DYLD_INSERT_LIBRARIES")
            .env_remove("DYLD_LIBRARY_PATH")
            .output()
            .await
            .with_context(|| {
                format!(
                    "failed to execute trusted listener source {}",
                    program.display()
                )
            })?;
        let text = String::from_utf8_lossy(&output.stdout);
        #[cfg(target_os = "windows")]
        let listeners = parse_windows_netstat(&text);
        #[cfg(target_os = "linux")]
        let listeners = parse_linux_ss(&text);
        #[cfg(target_os = "macos")]
        let listeners = parse_macos_lsof(&text);
        #[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
        let listeners = Vec::new();
        Ok(listeners)
    }
}

const MAX_DISCOVERY_REDIRECTS: usize = 2;

fn discovery_request_url_allowed(url: &reqwest::Url) -> bool {
    if !matches!(url.scheme(), "http" | "https")
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return false;
    }
    url.host_str().is_some_and(is_loopback_host)
}

#[cfg(any(target_os = "linux", target_os = "macos", test))]
fn first_existing_absolute(candidates: &[&str]) -> Option<PathBuf> {
    candidates
        .iter()
        .map(PathBuf::from)
        .find(|candidate| candidate.is_absolute() && candidate.is_file())
}

#[cfg(target_os = "windows")]
fn trusted_listener_command() -> Option<(PathBuf, Vec<&'static str>)> {
    let root = std::env::var_os("SystemRoot")
        .or_else(|| std::env::var_os("WINDIR"))
        .map(PathBuf::from)?;
    if !root.is_absolute() {
        return None;
    }
    let program = root.join("System32").join("netstat.exe");
    program
        .is_file()
        .then_some((program, vec!["-ano", "-p", "tcp"]))
}

#[cfg(target_os = "linux")]
fn trusted_listener_command() -> Option<(PathBuf, Vec<&'static str>)> {
    first_existing_absolute(&["/usr/bin/ss", "/bin/ss"])
        .map(|program| (program, vec!["-ltnpH"]))
}

#[cfg(target_os = "macos")]
fn trusted_listener_command() -> Option<(PathBuf, Vec<&'static str>)> {
    first_existing_absolute(&["/usr/sbin/lsof"])
        .map(|program| (program, vec!["-nP", "-iTCP", "-sTCP:LISTEN"]))
}

#[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
fn trusted_listener_command() -> Option<(PathBuf, Vec<&'static str>)> {
    None
}

pub struct HttpClassifier { client: Client }

impl HttpClassifier {
    pub fn new(timeout: Duration) -> Result<Self> {
        Ok(Self {
            client: Client::builder()
                .timeout(timeout)
                .redirect(reqwest::redirect::Policy::custom(|attempt| {
                    if !discovery_request_url_allowed(attempt.url()) {
                        return attempt.error(
                            "LocalView discovery redirect escaped the loopback HTTP(S) boundary",
                        );
                    }
                    if attempt.previous().len() > MAX_DISCOVERY_REDIRECTS {
                        return attempt.error("LocalView discovery redirect limit exceeded");
                    }
                    if let Some(initial) = attempt.previous().first()
                        && initial.origin() != attempt.url().origin()
                    {
                        return attempt.error(
                            "LocalView discovery redirect changed managed-surface origin",
                        );
                    }
                    attempt.follow()
                }))
                .build()?,
        })
    }

    pub async fn classify(&self, candidate: &ListenerCandidate) -> Result<Classification> {
        let url = candidate.endpoint.url()?;
        if !discovery_request_url_allowed(&url) {
            anyhow::bail!("LocalView discovery candidate is outside the loopback HTTP(S) boundary");
        }
        let response = self.client
            .get(url)
            .header("user-agent", "LocalView/0.2 discovery")
            .send()
            .await?;
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
        let candidates = listeners
            .into_iter()
            .filter(|c| is_loopback_host(&c.endpoint.host))
            .filter(|c| seen.insert((c.endpoint.host.clone(), c.endpoint.port)))
            .collect::<Vec<_>>();
        let results = stream::iter(candidates.into_iter().map(|candidate| async move {
            self.classifier.classify(&candidate).await.ok().map(|classification| DiscoveredServer { candidate, classification })
        }))
        .buffer_unordered(self.concurrency)
        .filter_map(|x| async move { x })
        .collect()
        .await;
        Ok(results)
    }
}

fn is_loopback_host(host: &str) -> bool {
    host == "localhost" || host == "::1" || host.parse::<IpAddr>().map(|ip| ip.is_loopback()).unwrap_or(false)
}

pub fn classify_response(status: u16, headers: &http::HeaderMap, body: &str) -> Classification {
    let lower = body.to_ascii_lowercase();
    let content_type = headers
        .get(http::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_ascii_lowercase();
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
    let kind = if framework.as_deref() == Some("Storybook") {
        ServerKind::Storybook
    } else if html && (framework.is_some() || hmr) {
        ServerKind::FrontendDevServer
    } else if html {
        ServerKind::StaticSite
    } else if api_like {
        ServerKind::ApiServer
    } else {
        ServerKind::UnknownHttp
    };
    let confidence = match kind {
        ServerKind::FrontendDevServer | ServerKind::Storybook => 0.98,
        ServerKind::StaticSite => 0.78,
        ServerKind::ApiServer => 0.88,
        ServerKind::UnknownHttp => 0.45,
    };
    Classification { kind, confidence, framework, title: extract_title(body), hmr_detected: hmr, evidence }
}

fn extract_title(body: &str) -> Option<String> {
    let re = Regex::new(r"(?is)<title[^>]*>(.*?)</title>").expect("static regex");
    re.captures(body)
        .and_then(|c| c.get(1))
        .map(|m| m.as_str().split_whitespace().collect::<Vec<_>>().join(" "))
        .filter(|s| !s.is_empty())
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
        Some(ListenerCandidate {
            endpoint: Endpoint { host, port, scheme: "http".into() },
            pid: parts.get(1).and_then(|x| x.parse().ok()),
            process_name: parts.first().map(|x| (*x).to_string()),
            command: None,
            cwd: None,
        })
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
    ListenerCandidate { endpoint: Endpoint { host, port, scheme: "http".into() }, pid, process_name: None, command: None, cwd: None }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn listener_source_never_resolves_tools_from_relative_path_entries() {
        for candidate in ["ss", "./ss", "netstat.exe", "../bin/lsof"] {
            assert!(
                first_existing_absolute(&[candidate]).is_none(),
                "relative listener executable must never be trusted: {candidate}"
            );
        }
    }

    #[test]
    fn parses_windows_listener() {
        let rows = parse_windows_netstat("  TCP    127.0.0.1:5173   0.0.0.0:0   LISTENING   4242\n");
        assert_eq!(rows[0].endpoint.port, 5173);
        assert_eq!(rows[0].pid, Some(4242));
    }

    #[test]
    fn detects_vite_frontend() {
        let mut headers = http::HeaderMap::new();
        headers.insert(http::header::CONTENT_TYPE, "text/html".parse().unwrap());
        let c = classify_response(200, &headers, "<html><title>App</title><script type=module src='/@vite/client'></script></html>");
        assert_eq!(c.kind, ServerKind::FrontendDevServer);
        assert_eq!(c.framework.as_deref(), Some("Vite"));
        assert!(c.hmr_detected);
    }

    #[test]
    fn detects_json_api() {
        let mut headers = http::HeaderMap::new();
        headers.insert(http::header::CONTENT_TYPE, "application/json".parse().unwrap());
        assert_eq!(classify_response(200, &headers, "{\"ok\":true}").kind, ServerKind::ApiServer);
    }

    fn fixture_candidate(port: u16) -> ListenerCandidate {
        ListenerCandidate {
            endpoint: Endpoint {
                host: "127.0.0.1".into(),
                port,
                scheme: "http".into(),
            },
            pid: None,
            process_name: None,
            command: None,
            cwd: None,
        }
    }

    fn spawn_http_fixture(
        responses: Vec<String>,
    ) -> (u16, std::thread::JoinHandle<()>) {
        use std::io::{Read, Write};
        use std::net::TcpListener;

        let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind fixture");
        let port = listener.local_addr().expect("fixture address").port();
        let handle = std::thread::spawn(move || {
            for response in responses {
                let (mut stream, _) = listener.accept().expect("accept fixture request");
                let mut request = [0_u8; 4096];
                let _ = stream.read(&mut request);
                stream
                    .write_all(response.as_bytes())
                    .expect("write fixture response");
            }
        });
        (port, handle)
    }

    fn redirect(location: &str) -> String {
        format!(
            "HTTP/1.1 302 Found\r\nLocation: {location}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
        )
    }

    fn html_ok() -> String {
        let body = "<!doctype html><title>Fixture</title>";
        format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        )
    }

    #[tokio::test]
    async fn discovery_redirect_cannot_escape_to_external_or_private_networks() {
        for target in ["http://192.0.2.10/escape", "http://10.0.0.10/private"] {
            let (port, server) = spawn_http_fixture(vec![redirect(target)]);
            let classifier = HttpClassifier::new(Duration::from_secs(2)).unwrap();
            classifier
                .classify(&fixture_candidate(port))
                .await
                .expect_err("redirect escape must fail before target request");
            server.join().unwrap();
        }
    }

    #[tokio::test]
    async fn discovery_redirect_allows_bounded_same_origin_chain() {
        let (port, server) = spawn_http_fixture(vec![redirect("/landing"), html_ok()]);
        let classifier = HttpClassifier::new(Duration::from_secs(2)).unwrap();
        let classification = classifier
            .classify(&fixture_candidate(port))
            .await
            .expect("same-origin loopback redirect should remain supported");
        assert_eq!(classification.kind, ServerKind::StaticSite);
        server.join().unwrap();
    }

    #[tokio::test]
    async fn discovery_redirect_rejects_loopback_origin_change() {
        let (port, server) = spawn_http_fixture(vec![redirect(
            "http://127.0.0.1:1/different-origin",
        )]);
        let classifier = HttpClassifier::new(Duration::from_secs(2)).unwrap();
        classifier
            .classify(&fixture_candidate(port))
            .await
            .expect_err("cross-port loopback redirect must fail before target request");
        server.join().unwrap();
    }

    #[tokio::test]
    async fn discovery_redirect_limit_and_scheme_change_fail_closed() {
        let (port, server) = spawn_http_fixture(vec![
            redirect("/one"),
            redirect("/two"),
            redirect("/three"),
        ]);
        let classifier = HttpClassifier::new(Duration::from_secs(2)).unwrap();
        classifier
            .classify(&fixture_candidate(port))
            .await
            .expect_err("third redirect must exceed bounded discovery policy");
        server.join().unwrap();

        let (scheme_port, scheme_server) =
            spawn_http_fixture(vec![redirect("ftp://127.0.0.1/not-http")]);
        classifier
            .classify(&fixture_candidate(scheme_port))
            .await
            .expect_err("scheme-changing redirect must fail");
        scheme_server.join().unwrap();
    }
}
