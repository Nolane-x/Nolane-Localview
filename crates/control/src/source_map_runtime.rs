use std::{
    ffi::OsString,
    path::{Component, Path, PathBuf},
};

use axum::{
    Json, Router,
    extract::{Path as AxumPath, State},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    routing::post,
};
use localview_protocol::SessionId;
use localview_source_map::{ResolvedSourceLocation, SourceMap};
use serde::{Deserialize, Serialize};
use tokio::fs;
use url::Url;

use crate::ControlState;

const MAX_REQUEST_PATH_BYTES: usize = 1_024;
const MAX_SOURCE_MAP_BYTES: u64 = 2 * 1024 * 1024;
const MAX_GENERATED_LINE: u32 = 1_000_000;
const MAX_GENERATED_COLUMN: u32 = 10_000_000;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ProjectSourceMapRequest {
    generated_file: String,
    generated_line: u32,
    generated_column: u32,
}

#[derive(Debug, Serialize)]
pub(crate) struct ProjectSourceMapResponse {
    generated_file: String,
    map_file: String,
    generated_line: u32,
    generated_column: u32,
    source: ProjectResolvedSource,
}

#[derive(Debug, Serialize)]
struct ProjectResolvedSource {
    file: String,
    line: u32,
    column: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<String>,
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum ProjectSourceMapError {
    Unauthorized,
    SessionNotFound,
    ProjectRootUnavailable,
    InvalidGeneratedFile,
    GeneratedFileUnavailable,
    SourceMapUnavailable,
    SourceMapTooLarge,
    InvalidSourceMap,
    GeneratedPositionOutOfRange,
    UnmappedGeneratedPosition,
    SourceReferenceUnsupported,
    SourceOutsideProject,
    SourceUnavailable,
}

impl ProjectSourceMapError {
    fn status(self) -> StatusCode {
        match self {
            Self::Unauthorized => StatusCode::UNAUTHORIZED,
            Self::SessionNotFound => StatusCode::NOT_FOUND,
            Self::ProjectRootUnavailable => StatusCode::CONFLICT,
            Self::GeneratedFileUnavailable
            | Self::SourceMapUnavailable
            | Self::SourceUnavailable => StatusCode::NOT_FOUND,
            Self::SourceOutsideProject => StatusCode::FORBIDDEN,
            Self::InvalidGeneratedFile
            | Self::SourceMapTooLarge
            | Self::InvalidSourceMap
            | Self::GeneratedPositionOutOfRange
            | Self::UnmappedGeneratedPosition
            | Self::SourceReferenceUnsupported => StatusCode::UNPROCESSABLE_ENTITY,
        }
    }

    fn code(self) -> &'static str {
        match self {
            Self::Unauthorized => "unauthorized",
            Self::SessionNotFound => "session_not_found",
            Self::ProjectRootUnavailable => "project_root_unavailable",
            Self::InvalidGeneratedFile => "invalid_generated_file",
            Self::GeneratedFileUnavailable => "generated_file_unavailable",
            Self::SourceMapUnavailable => "source_map_unavailable",
            Self::SourceMapTooLarge => "source_map_too_large",
            Self::InvalidSourceMap => "invalid_source_map",
            Self::GeneratedPositionOutOfRange => "generated_position_out_of_range",
            Self::UnmappedGeneratedPosition => "unmapped_generated_position",
            Self::SourceReferenceUnsupported => "source_reference_unsupported",
            Self::SourceOutsideProject => "source_outside_project",
            Self::SourceUnavailable => "source_unavailable",
        }
    }

    pub(crate) fn into_response(self) -> axum::response::Response {
        (
            self.status(),
            Json(serde_json::json!({ "error": self.code() })),
        )
            .into_response()
    }
}

pub(crate) fn router(state: ControlState) -> Router {
    Router::new()
        .route(
            "/v1/sessions/{id}/source-map/resolve",
            post(resolve_project_source_map),
        )
        .with_state(state)
}

async fn resolve_project_source_map(
    State(state): State<ControlState>,
    headers: HeaderMap,
    AxumPath(id): AxumPath<SessionId>,
    Json(request): Json<ProjectSourceMapRequest>,
) -> axum::response::Response {
    if !authorized(&headers, &state) {
        return ProjectSourceMapError::Unauthorized.into_response();
    }

    match resolve_project_source_map_inner(&state, id, request).await {
        Ok(response) => Json(response).into_response(),
        Err(error) => error.into_response(),
    }
}

pub(crate) async fn resolve_project_source_position(
    state: &ControlState,
    id: SessionId,
    generated_file: String,
    generated_line: u32,
    generated_column: u32,
) -> Result<ProjectSourceMapResponse, ProjectSourceMapError> {
    resolve_project_source_map_inner(
        state,
        id,
        ProjectSourceMapRequest {
            generated_file,
            generated_line,
            generated_column,
        },
    )
    .await
}

async fn resolve_project_source_map_inner(
    state: &ControlState,
    id: SessionId,
    request: ProjectSourceMapRequest,
) -> Result<ProjectSourceMapResponse, ProjectSourceMapError> {
    if request.generated_line == 0
        || request.generated_line > MAX_GENERATED_LINE
        || request.generated_column > MAX_GENERATED_COLUMN
    {
        return Err(ProjectSourceMapError::GeneratedPositionOutOfRange);
    }

    let generated_relative = bounded_project_relative_path(&request.generated_file)
        .ok_or(ProjectSourceMapError::InvalidGeneratedFile)?;

    let session = state
        .sessions
        .get(id)
        .await
        .ok_or(ProjectSourceMapError::SessionNotFound)?;
    let root_hint = session
        .project
        .git_root
        .as_deref()
        .or(session.project.cwd.as_deref())
        .ok_or(ProjectSourceMapError::ProjectRootUnavailable)?;

    let project_root = fs::canonicalize(root_hint)
        .await
        .map_err(|_| ProjectSourceMapError::ProjectRootUnavailable)?;
    let root_metadata = fs::metadata(&project_root)
        .await
        .map_err(|_| ProjectSourceMapError::ProjectRootUnavailable)?;
    if !root_metadata.is_dir() {
        return Err(ProjectSourceMapError::ProjectRootUnavailable);
    }

    let generated = fs::canonicalize(project_root.join(generated_relative))
        .await
        .map_err(|_| ProjectSourceMapError::GeneratedFileUnavailable)?;
    ensure_contained(&project_root, &generated)?;
    let generated_metadata = fs::metadata(&generated)
        .await
        .map_err(|_| ProjectSourceMapError::GeneratedFileUnavailable)?;
    if !generated_metadata.is_file() {
        return Err(ProjectSourceMapError::GeneratedFileUnavailable);
    }

    let map_candidate =
        sibling_map_path(&generated).ok_or(ProjectSourceMapError::SourceMapUnavailable)?;
    let map_path = fs::canonicalize(map_candidate)
        .await
        .map_err(|_| ProjectSourceMapError::SourceMapUnavailable)?;
    ensure_contained(&project_root, &map_path)?;
    let map_metadata = fs::metadata(&map_path)
        .await
        .map_err(|_| ProjectSourceMapError::SourceMapUnavailable)?;
    if !map_metadata.is_file() {
        return Err(ProjectSourceMapError::SourceMapUnavailable);
    }
    if map_metadata.len() > MAX_SOURCE_MAP_BYTES {
        return Err(ProjectSourceMapError::SourceMapTooLarge);
    }

    let map_bytes = fs::read(&map_path)
        .await
        .map_err(|_| ProjectSourceMapError::SourceMapUnavailable)?;
    if u64::try_from(map_bytes.len()).unwrap_or(u64::MAX) > MAX_SOURCE_MAP_BYTES {
        return Err(ProjectSourceMapError::SourceMapTooLarge);
    }
    let map_json =
        std::str::from_utf8(&map_bytes).map_err(|_| ProjectSourceMapError::InvalidSourceMap)?;
    let source_map =
        SourceMap::parse(map_json).map_err(|_| ProjectSourceMapError::InvalidSourceMap)?;
    let resolved = source_map
        .resolve(request.generated_line, request.generated_column)
        .ok_or(ProjectSourceMapError::UnmappedGeneratedPosition)?;

    let source_path = resolve_original_source_path(&project_root, &map_path, &resolved).await?;
    let generated_file = project_relative_display(&project_root, &generated)
        .ok_or(ProjectSourceMapError::InvalidGeneratedFile)?;
    let map_file = project_relative_display(&project_root, &map_path)
        .ok_or(ProjectSourceMapError::SourceMapUnavailable)?;
    let source_file = project_relative_display(&project_root, &source_path)
        .ok_or(ProjectSourceMapError::SourceUnavailable)?;

    Ok(ProjectSourceMapResponse {
        generated_file,
        map_file,
        generated_line: request.generated_line,
        generated_column: request.generated_column,
        source: ProjectResolvedSource {
            file: source_file,
            line: resolved.line,
            column: resolved.column,
            name: resolved.name,
        },
    })
}

async fn resolve_original_source_path(
    project_root: &Path,
    map_path: &Path,
    resolved: &ResolvedSourceLocation,
) -> Result<PathBuf, ProjectSourceMapError> {
    let reference = resolved.source.as_str();
    if reference.is_empty() || reference.len() > MAX_REQUEST_PATH_BYTES {
        return Err(ProjectSourceMapError::SourceReferenceUnsupported);
    }
    if reference.starts_with("//") {
        return Err(ProjectSourceMapError::SourceReferenceUnsupported);
    }

    let direct = PathBuf::from(reference);
    let candidate = if direct.is_absolute() {
        direct
    } else if let Ok(url) = Url::parse(reference) {
        if url.scheme() != "file" {
            return Err(ProjectSourceMapError::SourceReferenceUnsupported);
        }
        url.to_file_path()
            .map_err(|_| ProjectSourceMapError::SourceReferenceUnsupported)?
    } else {
        map_path
            .parent()
            .ok_or(ProjectSourceMapError::SourceMapUnavailable)?
            .join(direct)
    };

    let canonical = fs::canonicalize(candidate)
        .await
        .map_err(|_| ProjectSourceMapError::SourceUnavailable)?;
    ensure_contained(project_root, &canonical)?;
    let metadata = fs::metadata(&canonical)
        .await
        .map_err(|_| ProjectSourceMapError::SourceUnavailable)?;
    if !metadata.is_file() {
        return Err(ProjectSourceMapError::SourceUnavailable);
    }

    Ok(canonical)
}

fn bounded_project_relative_path(value: &str) -> Option<PathBuf> {
    if value.is_empty() || value.len() > MAX_REQUEST_PATH_BYTES {
        return None;
    }
    let path = Path::new(value);
    if path.is_absolute() {
        return None;
    }

    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Normal(value) => normalized.push(value),
            Component::CurDir => {}
            _ => return None,
        }
    }

    (!normalized.as_os_str().is_empty()).then_some(normalized)
}

fn sibling_map_path(generated: &Path) -> Option<PathBuf> {
    let file_name = generated.file_name()?;
    let mut map_name = OsString::from(file_name);
    map_name.push(".map");
    Some(generated.with_file_name(map_name))
}

fn ensure_contained(root: &Path, candidate: &Path) -> Result<(), ProjectSourceMapError> {
    if candidate.starts_with(root) {
        Ok(())
    } else {
        Err(ProjectSourceMapError::SourceOutsideProject)
    }
}

fn project_relative_display(root: &Path, path: &Path) -> Option<String> {
    let relative = path.strip_prefix(root).ok()?;
    let value = relative.to_str()?.replace('\\', "/");
    if value.is_empty() || value.len() > MAX_REQUEST_PATH_BYTES {
        return None;
    }
    Some(value)
}

fn authorized(headers: &HeaderMap, state: &ControlState) -> bool {
    let expected = format!("Bearer {}", state.token);
    headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value == expected)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn caller_path_must_be_bounded_and_project_relative() {
        assert_eq!(
            bounded_project_relative_path("dist/./app.js"),
            Some(PathBuf::from("dist/app.js"))
        );
        assert!(bounded_project_relative_path("../outside.js").is_none());
        assert!(bounded_project_relative_path("").is_none());
        assert!(bounded_project_relative_path(&"a".repeat(MAX_REQUEST_PATH_BYTES + 1)).is_none());
    }

    #[test]
    fn sibling_map_name_is_deterministic() {
        assert_eq!(
            sibling_map_path(Path::new("/tmp/project/dist/app.js")),
            Some(PathBuf::from("/tmp/project/dist/app.js.map"))
        );
    }
}
