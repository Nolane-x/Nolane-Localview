#![forbid(unsafe_code)]

use std::collections::BTreeMap;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use smallvec::SmallVec;
use url::Url;
use uuid::Uuid;

pub type SessionId = Uuid;
pub type ElementRef = String;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct Endpoint {
    pub host: String,
    pub port: u16,
    pub scheme: String,
}

impl Endpoint {
    pub fn url(&self) -> Result<Url, url::ParseError> {
        Url::parse(&format!("{}://{}:{}/", self.scheme, self.host, self.port))
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ServerKind {
    FrontendDevServer,
    Storybook,
    ApiServer,
    StaticSite,
    UnknownHttp,
}

impl ServerKind {
    pub fn visual_candidate(self) -> bool {
        matches!(self, Self::FrontendDevServer | Self::Storybook | Self::StaticSite)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Classification {
    pub kind: ServerKind,
    pub confidence: f32,
    pub framework: Option<String>,
    pub title: Option<String>,
    pub hmr_detected: bool,
    pub evidence: SmallVec<[String; 6]>,
}

impl Default for Classification {
    fn default() -> Self {
        Self { kind: ServerKind::UnknownHttp, confidence: 0.0, framework: None, title: None, hmr_detected: false, evidence: SmallVec::new() }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SessionStatus { Active, Disconnected, Hidden, Closed }

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Session {
    pub id: SessionId,
    pub endpoint: Endpoint,
    pub classification: Classification,
    pub project: ProjectIdentity,
    pub status: SessionStatus,
    pub first_seen: DateTime<Utc>,
    pub last_seen: DateTime<Utc>,
    pub disconnected_at: Option<DateTime<Utc>>,
    pub preview_visible: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct ProjectIdentity {
    pub key: String,
    pub display_name: String,
    pub cwd: Option<String>,
    pub git_root: Option<String>,
    pub pid: Option<u32>,
    pub command: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ListenerCandidate {
    pub endpoint: Endpoint,
    pub pid: Option<u32>,
    pub process_name: Option<String>,
    pub command: Option<String>,
    pub cwd: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DiscoveredServer {
    pub candidate: ListenerCandidate,
    pub classification: Classification,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Rect { pub x: f64, pub y: f64, pub width: f64, pub height: f64 }

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ViewportMeta {
    pub css_width: u32,
    pub css_height: u32,
    pub device_scale_factor: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum VisualChangeExpectation {
    Unchanged { max_changed_ratio: f64 },
    Changed { min_changed_ratio: f64 },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub struct VisualDiffMetrics {
    pub changed_pixels: u64,
    pub changed_ratio: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SemanticNode {
    pub reference: ElementRef,
    pub role: Option<String>,
    pub name: Option<String>,
    pub tag: String,
    pub rect: Option<Rect>,
    pub interactive: bool,
    pub attributes: BTreeMap<String, String>,
    pub source: Option<SourceLocation>,
    pub children: Vec<SemanticNode>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SourceLocation { pub file: String, pub line: u32, pub column: Option<u32>, pub component: Option<String> }

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PageSnapshot {
    pub version: u64,
    pub route: String,
    pub viewport: (u32, u32),
    pub root: SemanticNode,
    pub console_errors: Vec<ConsoleIssue>,
    pub failed_requests: Vec<NetworkIssue>,
    pub captured_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ConsoleIssue { pub level: String, pub message: String, pub source: Option<String>, pub count: u32 }

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct NetworkIssue { pub method: String, pub url: String, pub status: Option<u16>, pub error: Option<String> }

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct StateDiff {
    pub from_version: u64,
    pub to_version: u64,
    pub changed_refs: Vec<ElementRef>,
    pub removed_refs: Vec<ElementRef>,
    pub route_changed: bool,
    pub layout_changes: Vec<LayoutChange>,
    pub console_delta: Vec<ConsoleIssue>,
    pub network_delta: Vec<NetworkIssue>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LayoutChange { pub reference: ElementRef, pub before: Option<Rect>, pub after: Option<Rect> }

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ObservationEvent {
    ServerDetected { session_id: SessionId, endpoint: Endpoint },
    ServerDisconnected { session_id: SessionId },
    ServerReconnected { session_id: SessionId },
    DomChanged { session_id: SessionId, refs: Vec<ElementRef> },
    LayoutChanged { session_id: SessionId, refs: Vec<ElementRef> },
    RouteChanged { session_id: SessionId, route: String },
    ConsoleIssue { session_id: SessionId, issue: ConsoleIssue },
    NetworkIssue { session_id: SessionId, issue: NetworkIssue },
    HmrStarted { session_id: SessionId },
    HmrSettled { session_id: SessionId },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum Capability { Observe, Interact, Test, Advanced }

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TokenBudget { pub max_tokens: usize, pub detail: DetailLevel }

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DetailLevel { Minimal, Normal, Deep }

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Health { pub version: String, pub status: String, pub paused: bool, pub sessions: usize }

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn visual_candidate_rules_are_conservative() {
        assert!(ServerKind::FrontendDevServer.visual_candidate());
        assert!(ServerKind::Storybook.visual_candidate());
        assert!(!ServerKind::ApiServer.visual_candidate());
    }
}
