use localview_protocol::{Endpoint, ProjectIdentity, ServerKind};
use serde::{Deserialize, Serialize};
use thiserror::Error;

pub const MAX_NORMALIZED_PROJECT_PATH_BYTES: usize = 4_096;
pub const MAX_ENDPOINT_HOST_BYTES: usize = 255;
pub const MAX_ENDPOINT_SCHEME_BYTES: usize = 32;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(tag = "lineage_version", content = "value")]
pub enum SessionLineage {
    #[serde(rename = "localview_session_lineage_v1")]
    V1(SessionLineageV1),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct SessionLineageV1 {
    pub anchor: SessionLineageAnchorV1,
    pub server_kind: SessionServerKind,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(tag = "anchor_type", rename_all = "snake_case")]
pub enum SessionLineageAnchorV1 {
    Project {
        normalized_project_path: String,
    },
    Endpoint {
        scheme: String,
        host: String,
        port: u16,
    },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum SessionServerKind {
    FrontendDevServer,
    Storybook,
    ApiServer,
    StaticSite,
    UnknownHttp,
}

impl From<ServerKind> for SessionServerKind {
    fn from(value: ServerKind) -> Self {
        match value {
            ServerKind::FrontendDevServer => Self::FrontendDevServer,
            ServerKind::Storybook => Self::Storybook,
            ServerKind::ApiServer => Self::ApiServer,
            ServerKind::StaticSite => Self::StaticSite,
            ServerKind::UnknownHttp => Self::UnknownHttp,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum SessionIdentityError {
    #[error("project path is empty or exceeds the durable identity limit")]
    InvalidProjectPath,
    #[error("project path contains unresolved parent traversal")]
    ParentTraversal,
    #[error("endpoint scheme is invalid for durable identity")]
    InvalidEndpointScheme,
    #[error("endpoint host is invalid for durable identity")]
    InvalidEndpointHost,
}

pub fn session_lineage(
    project: &ProjectIdentity,
    endpoint: &Endpoint,
    kind: ServerKind,
) -> Result<SessionLineage, SessionIdentityError> {
    let anchor = match project.git_root.as_deref().or(project.cwd.as_deref()) {
        Some(path) if !path.is_empty() => SessionLineageAnchorV1::Project {
            normalized_project_path: normalize_project_path(path)?,
        },
        _ => SessionLineageAnchorV1::Endpoint {
            scheme: validate_endpoint_part(
                &endpoint.scheme,
                MAX_ENDPOINT_SCHEME_BYTES,
                SessionIdentityError::InvalidEndpointScheme,
            )?,
            host: validate_endpoint_part(
                &endpoint.host,
                MAX_ENDPOINT_HOST_BYTES,
                SessionIdentityError::InvalidEndpointHost,
            )?,
            port: endpoint.port,
        },
    };

    Ok(SessionLineage::V1(SessionLineageV1 {
        anchor,
        server_kind: kind.into(),
    }))
}

fn validate_endpoint_part(
    value: &str,
    max_bytes: usize,
    error: SessionIdentityError,
) -> Result<String, SessionIdentityError> {
    if value.is_empty() || value.len() > max_bytes || value.chars().any(char::is_control) {
        return Err(error);
    }
    Ok(value.to_owned())
}

fn normalize_project_path(raw: &str) -> Result<String, SessionIdentityError> {
    normalize_project_path_for_flavor(raw, PathFlavor::current())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PathFlavor {
    Unix,
    Windows,
}

impl PathFlavor {
    const fn current() -> Self {
        if cfg!(windows) {
            Self::Windows
        } else {
            Self::Unix
        }
    }
}

fn normalize_project_path_for_flavor(
    raw: &str,
    flavor: PathFlavor,
) -> Result<String, SessionIdentityError> {
    if raw.is_empty()
        || raw.len() > MAX_NORMALIZED_PROJECT_PATH_BYTES
        || raw.chars().any(char::is_control)
    {
        return Err(SessionIdentityError::InvalidProjectPath);
    }

    match flavor {
        PathFlavor::Unix => normalize_unix_path(raw),
        PathFlavor::Windows => normalize_windows_path(raw),
    }
}

fn normalized_components<'a>(
    components: impl Iterator<Item = &'a str>,
) -> Result<Vec<&'a str>, SessionIdentityError> {
    let mut normalized = Vec::new();
    for component in components {
        match component {
            "" | "." => {}
            ".." => return Err(SessionIdentityError::ParentTraversal),
            value => normalized.push(value),
        }
    }
    Ok(normalized)
}

fn normalize_unix_path(raw: &str) -> Result<String, SessionIdentityError> {
    let rooted = raw.starts_with('/');
    let components = normalized_components(raw.split('/'))?;
    if components.is_empty() {
        return if rooted {
            Ok("/".into())
        } else {
            Err(SessionIdentityError::InvalidProjectPath)
        };
    }

    let joined = components.join("/");
    Ok(if rooted {
        format!("/{joined}")
    } else {
        joined
    })
}

fn normalize_windows_path(raw: &str) -> Result<String, SessionIdentityError> {
    let replaced = raw.replace('\\', "/").to_lowercase();
    let bytes = replaced.as_bytes();
    let is_unc = replaced.starts_with("//");
    let has_drive = bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':';

    let (prefix, rest) = if is_unc {
        ("//".to_owned(), replaced.trim_start_matches('/'))
    } else if has_drive {
        let drive = &replaced[..2];
        let after_drive = &replaced[2..];
        if after_drive.starts_with('/') {
            (format!("{drive}/"), after_drive.trim_start_matches('/'))
        } else {
            (drive.to_owned(), after_drive)
        }
    } else if replaced.starts_with('/') {
        ("/".to_owned(), replaced.trim_start_matches('/'))
    } else {
        (String::new(), replaced.as_str())
    };

    let components = normalized_components(rest.split('/'))?;
    if components.is_empty() {
        return if prefix.is_empty() {
            Err(SessionIdentityError::InvalidProjectPath)
        } else {
            Ok(prefix)
        };
    }

    Ok(format!("{prefix}{}", components.join("/")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unix_normalization_is_lexical_and_case_preserving() {
        assert_eq!(
            normalize_project_path_for_flavor("/Work/./app//", PathFlavor::Unix).unwrap(),
            "/Work/app"
        );
        assert_ne!(
            normalize_project_path_for_flavor("/Work/app", PathFlavor::Unix).unwrap(),
            normalize_project_path_for_flavor("/work/app", PathFlavor::Unix).unwrap()
        );
    }

    #[test]
    fn windows_normalization_is_separator_and_case_stable() {
        let backslash = normalize_project_path_for_flavor(
            r"C:\\Users\\Dev\\App\\",
            PathFlavor::Windows,
        )
        .unwrap();
        let slash = normalize_project_path_for_flavor(
            "c:/users/dev/app",
            PathFlavor::Windows,
        )
        .unwrap();
        assert_eq!(backslash, "c:/users/dev/app");
        assert_eq!(backslash, slash);
    }

    #[test]
    fn unresolved_parent_is_rejected_for_both_flavors() {
        assert_eq!(
            normalize_project_path_for_flavor("/work/../app", PathFlavor::Unix),
            Err(SessionIdentityError::ParentTraversal)
        );
        assert_eq!(
            normalize_project_path_for_flavor(r"C:\\work\\..\\app", PathFlavor::Windows),
            Err(SessionIdentityError::ParentTraversal)
        );
    }
}
