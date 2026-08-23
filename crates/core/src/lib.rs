#![forbid(unsafe_code)]

use std::{collections::hash_map::DefaultHasher, hash::{Hash, Hasher}, path::{Path, PathBuf}, time::Duration};
use localview_protocol::{ListenerCandidate, ProjectIdentity};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeConfig {
    pub scan_interval: Duration,
    pub disconnect_grace: Duration,
    pub probe_timeout: Duration,
    pub probe_concurrency: usize,
    pub control_port: u16,
    pub auto_open: AutoOpenMode,
}

impl Default for RuntimeConfig {
    fn default() -> Self {
        Self {
            scan_interval: Duration::from_millis(750),
            disconnect_grace: Duration::from_secs(3),
            probe_timeout: Duration::from_millis(900),
            probe_concurrency: 24,
            control_port: 45454,
            auto_open: AutoOpenMode::Notify,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AutoOpenMode { AutoOpen, Notify, Silent, Paused }

pub fn project_identity(candidate: &ListenerCandidate) -> ProjectIdentity {
    let root = candidate.cwd.as_deref().map(git_root_or_cwd);
    let mut hasher = DefaultHasher::new();
    root.as_deref().unwrap_or("").hash(&mut hasher);
    candidate.command.as_deref().unwrap_or("").hash(&mut hasher);
    let key = format!("project-{:016x}", hasher.finish());
    let display_name = root.as_deref()
        .and_then(|p| Path::new(p).file_name())
        .and_then(|p| p.to_str())
        .filter(|s| !s.is_empty())
        .unwrap_or("localhost")
        .to_owned();
    ProjectIdentity {
        key,
        display_name,
        cwd: candidate.cwd.clone(),
        git_root: root,
        pid: candidate.pid,
        command: candidate.command.clone(),
    }
}

fn git_root_or_cwd(cwd: &str) -> String {
    let mut cursor = PathBuf::from(cwd);
    loop {
        if cursor.join(".git").exists() { return cursor.to_string_lossy().into_owned(); }
        if !cursor.pop() { break; }
    }
    cwd.to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use localview_protocol::Endpoint;
    #[test]
    fn stable_identity_ignores_port() {
        let base = ListenerCandidate { endpoint: Endpoint { host:"127.0.0.1".into(), port:5173, scheme:"http".into() }, pid:Some(5), process_name:None, command:Some("vite".into()), cwd:Some("/tmp/app".into()) };
        let mut moved = base.clone(); moved.endpoint.port = 5174;
        assert_eq!(project_identity(&base).key, project_identity(&moved).key);
    }
}
