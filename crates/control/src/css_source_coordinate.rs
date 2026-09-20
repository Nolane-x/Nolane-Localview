use std::{
    ffi::OsString,
    path::{Component, Path, PathBuf},
};

use localview_protocol::SessionId;
use localview_source_map::{ResolvedSourceLocation, SourceMap};
use serde::Serialize;
use tokio::fs;
use url::Url;

use crate::ControlState;

use super::{CssCascadeWinner, CssDeclarationEvidence, CssStyleTrace};

const MAX_CSS_FILE_BYTES: u64 = 512 * 1024;
const MAX_CSS_SOURCE_MAP_BYTES: u64 = 2 * 1024 * 1024;
const MAX_PARSED_RULES: usize = 512;
const MAX_PARSED_DECLARATIONS: usize = 4_096;
const MAX_PARSE_DEPTH: usize = 12;
const MAX_COMPONENT_DEPTH: usize = 32;
const MAX_SELECTOR_BYTES: usize = 256;
const MAX_PROPERTY_BYTES: usize = 64;
const MAX_SOURCE_VALUE_BYTES: usize = 1_024;
const MAX_PATH_BYTES: usize = 1_024;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub(crate) struct CssSourceAuthority {
    pub level: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub column: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mapping: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub coordinate_space: Option<String>,
}

impl CssSourceAuthority {
    pub(crate) fn initial(source_kind: &str) -> Self {
        match source_kind {
            "same_origin_stylesheet" => Self::stylesheet_hint(),
            "inline_element" | "inline_stylesheet" => Self::runtime_inline(),
            _ => Self::unresolved(),
        }
    }

    fn unresolved() -> Self {
        Self {
            level: "unresolved".into(),
            file: None,
            line: None,
            column: None,
            mapping: None,
            coordinate_space: None,
        }
    }

    fn stylesheet_hint() -> Self {
        Self {
            level: "stylesheet_hint".into(),
            file: None,
            line: None,
            column: None,
            mapping: None,
            coordinate_space: None,
        }
    }

    fn runtime_inline() -> Self {
        Self {
            level: "runtime_inline".into(),
            file: None,
            line: None,
            column: None,
            mapping: None,
            coordinate_space: None,
        }
    }

    fn project_file_verified(file: String) -> Self {
        Self {
            level: "project_file_verified".into(),
            file: Some(file),
            line: None,
            column: None,
            mapping: None,
            coordinate_space: None,
        }
    }

    fn exact(
        file: String,
        line: u32,
        column: u32,
        mapping: &'static str,
        coordinate_space: &'static str,
    ) -> Self {
        Self {
            level: "exact_declaration_position".into(),
            file: Some(file),
            line: Some(line),
            column: Some(column),
            mapping: Some(mapping.into()),
            coordinate_space: Some(coordinate_space.into()),
        }
    }
}

#[derive(Debug, Clone)]
struct CssEvidenceQuery {
    source_kind: String,
    stylesheet_path: Option<String>,
    selector: Option<String>,
    property: String,
    value: String,
    important: bool,
}

impl CssEvidenceQuery {
    fn declaration(value: &CssDeclarationEvidence) -> Self {
        Self {
            source_kind: value.source_kind.clone(),
            stylesheet_path: value.stylesheet_path.clone(),
            selector: value.selector.clone(),
            property: value.property.clone(),
            value: value.value.clone(),
            important: value.important,
        }
    }

    fn winner(value: &CssCascadeWinner) -> Self {
        Self {
            source_kind: value.source_kind.clone(),
            stylesheet_path: value.stylesheet_path.clone(),
            selector: value.selector.clone(),
            property: value.property.clone(),
            value: value.value.clone(),
            important: value.important,
        }
    }
}

#[derive(Debug)]
struct ProjectCssContext {
    root: PathBuf,
}

impl ProjectCssContext {
    async fn for_session(state: &ControlState, id: SessionId) -> Option<Self> {
        let session = state.sessions.get(id).await?;
        let root_hint = session
            .project
            .git_root
            .as_deref()
            .or(session.project.cwd.as_deref())?;
        let root = fs::canonicalize(root_hint).await.ok()?;
        let metadata = fs::metadata(&root).await.ok()?;
        metadata.is_dir().then_some(Self { root })
    }
}

pub(crate) async fn enrich_style_trace_source_authority(
    state: &ControlState,
    id: SessionId,
    trace: &mut CssStyleTrace,
) {
    let Some(context) = ProjectCssContext::for_session(state, id).await else {
        return;
    };

    for declaration in &mut trace.declarations {
        let query = CssEvidenceQuery::declaration(declaration);
        declaration.source_authority = resolve_query(&context, query).await;
    }

    if let Some(cascade) = &mut trace.author_cascade {
        for winner in &mut cascade.winners {
            let query = CssEvidenceQuery::winner(winner);
            winner.source_authority = resolve_query(&context, query).await;
        }
    }
}

async fn resolve_query(context: &ProjectCssContext, query: CssEvidenceQuery) -> CssSourceAuthority {
    if query.source_kind != "same_origin_stylesheet" {
        return CssSourceAuthority::initial(&query.source_kind);
    }

    let Some(path_hint) = query.stylesheet_path.as_deref() else {
        return CssSourceAuthority::stylesheet_hint();
    };
    let Some(selector) = query.selector.as_deref() else {
        return CssSourceAuthority::stylesheet_hint();
    };
    let Some(relative) = bounded_project_relative_path(path_hint) else {
        return CssSourceAuthority::stylesheet_hint();
    };

    let canonical = match fs::canonicalize(context.root.join(relative)).await {
        Ok(path) => path,
        Err(_) => return CssSourceAuthority::stylesheet_hint(),
    };
    if !canonical.starts_with(&context.root) {
        return CssSourceAuthority::stylesheet_hint();
    }

    let metadata = match fs::metadata(&canonical).await {
        Ok(metadata) if metadata.is_file() => metadata,
        _ => return CssSourceAuthority::stylesheet_hint(),
    };
    let Some(verified_file) = project_relative_display(&context.root, &canonical) else {
        return CssSourceAuthority::stylesheet_hint();
    };
    let verified = CssSourceAuthority::project_file_verified(verified_file.clone());

    if metadata.len() > MAX_CSS_FILE_BYTES {
        return verified;
    }
    if canonical
        .extension()
        .and_then(|value| value.to_str())
        .is_none_or(|value| !value.eq_ignore_ascii_case("css"))
    {
        return verified;
    }

    let bytes = match fs::read(&canonical).await {
        Ok(bytes) if u64::try_from(bytes.len()).unwrap_or(u64::MAX) <= MAX_CSS_FILE_BYTES => bytes,
        _ => return verified,
    };
    let source = match std::str::from_utf8(&bytes) {
        Ok(source) => source,
        Err(_) => return verified,
    };

    let parsed = match parse_stylesheet(source) {
        Ok(parsed) => parsed,
        Err(_) => return verified,
    };
    let Some(expected_selector) = normalize_selector(selector) else {
        return verified;
    };
    let Some(expected_value) = normalize_css_value(&query.value) else {
        return verified;
    };

    let mut matched = parsed.into_iter().filter(|candidate| {
        candidate.selector == expected_selector
            && candidate.property.eq_ignore_ascii_case(&query.property)
            && candidate.value == expected_value
            && candidate.important == query.important
    });
    let Some(location) = matched.next() else {
        return verified;
    };
    if matched.next().is_some() {
        return verified;
    }

    let direct = CssSourceAuthority::exact(
        verified_file.clone(),
        location.line,
        location.column,
        "direct_css",
        "line_1_based_column_unicode_scalar_0_based",
    );

    mapped_source_authority(
        context,
        &canonical,
        location.line,
        location.source_map_column,
    )
    .await
    .unwrap_or(direct)
}

async fn mapped_source_authority(
    context: &ProjectCssContext,
    generated: &Path,
    generated_line: u32,
    generated_column: u32,
) -> Option<CssSourceAuthority> {
    let map_candidate = sibling_map_path(generated)?;
    let map_path = fs::canonicalize(map_candidate).await.ok()?;
    if !map_path.starts_with(&context.root) {
        return None;
    }

    let metadata = fs::metadata(&map_path).await.ok()?;
    if !metadata.is_file() || metadata.len() > MAX_CSS_SOURCE_MAP_BYTES {
        return None;
    }

    let bytes = fs::read(&map_path).await.ok()?;
    if u64::try_from(bytes.len()).ok()? > MAX_CSS_SOURCE_MAP_BYTES {
        return None;
    }
    let map_json = std::str::from_utf8(&bytes).ok()?;
    let source_map = SourceMap::parse(map_json).ok()?;

    // CSS exact-source authority requires an actual mapping segment at the
    // declaration coordinate. Nearest-preceding Source Map lookup is useful
    // for runtime stacks, but is insufficient proof for this lane.
    let resolved = source_map.resolve_exact(generated_line, generated_column)?;
    let source_path = resolve_original_source_path(&context.root, &map_path, &resolved).await?;
    let file = project_relative_display(&context.root, &source_path)?;

    Some(CssSourceAuthority::exact(
        file,
        resolved.line,
        resolved.column,
        "project_source_map",
        "source_map_v3_original_line_1_based_column_0_based",
    ))
}

fn sibling_map_path(generated: &Path) -> Option<PathBuf> {
    let file_name = generated.file_name()?;
    let mut map_name = OsString::from(file_name);
    map_name.push(".map");
    Some(generated.with_file_name(map_name))
}

async fn resolve_original_source_path(
    project_root: &Path,
    map_path: &Path,
    resolved: &ResolvedSourceLocation,
) -> Option<PathBuf> {
    let reference = resolved.source.as_str();
    if reference.is_empty() || reference.len() > MAX_PATH_BYTES || reference.starts_with("//") {
        return None;
    }

    let direct = PathBuf::from(reference);
    let candidate = if direct.is_absolute() {
        direct
    } else if let Ok(url) = Url::parse(reference) {
        if url.scheme() != "file" {
            return None;
        }
        url.to_file_path().ok()?
    } else {
        map_path.parent()?.join(direct)
    };

    let canonical = fs::canonicalize(candidate).await.ok()?;
    if !canonical.starts_with(project_root) {
        return None;
    }
    let metadata = fs::metadata(&canonical).await.ok()?;
    metadata.is_file().then_some(canonical)
}

fn bounded_project_relative_path(value: &str) -> Option<PathBuf> {
    if value.is_empty() || value.len() > MAX_PATH_BYTES || value.contains('%') {
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

fn project_relative_display(root: &Path, path: &Path) -> Option<String> {
    let relative = path.strip_prefix(root).ok()?;
    let value = relative.to_str()?.replace('\\', "/");
    if value.is_empty() || value.len() > MAX_PATH_BYTES {
        return None;
    }
    bounded_project_relative_path(&value)?;
    Some(value)
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ParsedDeclaration {
    selector: String,
    property: String,
    value: String,
    important: bool,
    line: u32,
    column: u32,
    source_map_column: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CssParseError {
    Invalid,
    Unsupported,
    Limit,
}

#[derive(Default)]
struct ParseBudget {
    rules: usize,
    declarations: usize,
}

fn parse_stylesheet(source: &str) -> Result<Vec<ParsedDeclaration>, CssParseError> {
    if u64::try_from(source.len()).unwrap_or(u64::MAX) > MAX_CSS_FILE_BYTES {
        return Err(CssParseError::Limit);
    }
    let mut output = Vec::new();
    let mut budget = ParseBudget::default();
    parse_rule_list(source, 0, source.len(), 0, &mut budget, &mut output)?;
    Ok(output)
}

fn parse_rule_list(
    source: &str,
    mut cursor: usize,
    end: usize,
    depth: usize,
    budget: &mut ParseBudget,
    output: &mut Vec<ParsedDeclaration>,
) -> Result<(), CssParseError> {
    if depth > MAX_PARSE_DEPTH {
        return Err(CssParseError::Limit);
    }

    while cursor < end {
        skip_ws_comments(source, &mut cursor, end)?;
        if cursor >= end {
            break;
        }
        let start = cursor;
        let boundary = find_rule_boundary(source, cursor, end)?;
        match boundary {
            RuleBoundary::Semicolon(position) => {
                let prelude = normalize_prelude(&source[start..position])?;
                if !prelude.starts_with('@') {
                    return Err(CssParseError::Unsupported);
                }
                budget.rules = budget.rules.checked_add(1).ok_or(CssParseError::Limit)?;
                if budget.rules > MAX_PARSED_RULES {
                    return Err(CssParseError::Limit);
                }
                let name = at_rule_name(&prelude).ok_or(CssParseError::Unsupported)?;
                if !name.eq_ignore_ascii_case("charset") {
                    return Err(CssParseError::Unsupported);
                }
                cursor = position + 1;
            }
            RuleBoundary::Block(open) => {
                budget.rules = budget.rules.checked_add(1).ok_or(CssParseError::Limit)?;
                if budget.rules > MAX_PARSED_RULES {
                    return Err(CssParseError::Limit);
                }
                let close = find_matching_brace(source, open, end)?;
                let prelude = normalize_prelude(&source[start..open])?;
                if prelude.starts_with('@') {
                    let name = at_rule_name(&prelude).ok_or(CssParseError::Unsupported)?;
                    match name.to_ascii_lowercase().as_str() {
                        "media" | "supports" => {
                            parse_rule_list(source, open + 1, close, depth + 1, budget, output)?;
                        }
                        "font-face"
                        | "keyframes"
                        | "-webkit-keyframes"
                        | "page"
                        | "property"
                        | "counter-style"
                        | "font-feature-values" => {}
                        _ => return Err(CssParseError::Unsupported),
                    }
                } else {
                    let selector =
                        normalize_selector(&prelude).ok_or(CssParseError::Unsupported)?;
                    parse_declarations(source, open + 1, close, &selector, budget, output)?;
                }
                cursor = close + 1;
            }
            RuleBoundary::End => {
                if source[start..end].trim().is_empty() {
                    break;
                }
                return Err(CssParseError::Invalid);
            }
        }
    }
    Ok(())
}

enum RuleBoundary {
    Semicolon(usize),
    Block(usize),
    End,
}

fn find_rule_boundary(
    source: &str,
    mut cursor: usize,
    end: usize,
) -> Result<RuleBoundary, CssParseError> {
    let bytes = source.as_bytes();
    let mut quote = None;
    let mut paren = 0_usize;
    let mut bracket = 0_usize;

    while cursor < end {
        let byte = bytes[cursor];
        if let Some(mark) = quote {
            if byte == b'\\' {
                cursor = cursor.checked_add(2).ok_or(CssParseError::Limit)?;
                continue;
            }
            if byte == mark {
                quote = None;
            }
            cursor += 1;
            continue;
        }
        if byte == b'/' && cursor + 1 < end && bytes[cursor + 1] == b'*' {
            cursor = skip_comment(source, cursor, end)?;
            continue;
        }
        match byte {
            b'\'' | b'"' => quote = Some(byte),
            b'(' => {
                paren += 1;
                if paren > MAX_COMPONENT_DEPTH {
                    return Err(CssParseError::Limit);
                }
            }
            b')' => {
                paren = paren.checked_sub(1).ok_or(CssParseError::Invalid)?;
            }
            b'[' => {
                bracket += 1;
                if bracket > MAX_COMPONENT_DEPTH {
                    return Err(CssParseError::Limit);
                }
            }
            b']' => {
                bracket = bracket.checked_sub(1).ok_or(CssParseError::Invalid)?;
            }
            b';' if paren == 0 && bracket == 0 => return Ok(RuleBoundary::Semicolon(cursor)),
            b'{' if paren == 0 && bracket == 0 => return Ok(RuleBoundary::Block(cursor)),
            b'}' if paren == 0 && bracket == 0 => return Err(CssParseError::Invalid),
            _ => {}
        }
        cursor += 1;
    }
    if quote.is_some() || paren != 0 || bracket != 0 {
        Err(CssParseError::Invalid)
    } else {
        Ok(RuleBoundary::End)
    }
}

fn find_matching_brace(source: &str, open: usize, end: usize) -> Result<usize, CssParseError> {
    let bytes = source.as_bytes();
    let mut cursor = open + 1;
    let mut quote = None;
    let mut depth = 1_usize;

    while cursor < end {
        let byte = bytes[cursor];
        if let Some(mark) = quote {
            if byte == b'\\' {
                cursor = cursor.checked_add(2).ok_or(CssParseError::Limit)?;
                continue;
            }
            if byte == mark {
                quote = None;
            }
            cursor += 1;
            continue;
        }
        if byte == b'/' && cursor + 1 < end && bytes[cursor + 1] == b'*' {
            cursor = skip_comment(source, cursor, end)?;
            continue;
        }
        match byte {
            b'\'' | b'"' => quote = Some(byte),
            b'{' => {
                depth += 1;
                if depth > MAX_PARSE_DEPTH + 2 {
                    return Err(CssParseError::Limit);
                }
            }
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return Ok(cursor);
                }
            }
            _ => {}
        }
        cursor += 1;
    }
    Err(CssParseError::Invalid)
}

fn parse_declarations(
    source: &str,
    mut cursor: usize,
    end: usize,
    selector: &str,
    budget: &mut ParseBudget,
    output: &mut Vec<ParsedDeclaration>,
) -> Result<(), CssParseError> {
    while cursor < end {
        skip_ws_comments(source, &mut cursor, end)?;
        if cursor >= end {
            break;
        }
        let segment_start = cursor;
        let segment_end = find_declaration_end(source, cursor, end)?;
        let Some(colon) = find_top_level_colon(source, segment_start, segment_end)? else {
            if source[segment_start..segment_end].trim().is_empty() {
                cursor = segment_end.saturating_add(1);
                continue;
            }
            return Err(CssParseError::Unsupported);
        };

        let property = normalize_property(&source[segment_start..colon])?;
        let raw_value = &source[colon + 1..segment_end];
        let (value, important) = normalize_value_and_priority(raw_value)?;
        let property_offset =
            first_non_ws_comment(source, segment_start, colon)?.ok_or(CssParseError::Invalid)?;
        let (line, column, source_map_column) = line_columns(source, property_offset)?;

        budget.declarations = budget
            .declarations
            .checked_add(1)
            .ok_or(CssParseError::Limit)?;
        if budget.declarations > MAX_PARSED_DECLARATIONS {
            return Err(CssParseError::Limit);
        }

        output.push(ParsedDeclaration {
            selector: selector.to_owned(),
            property,
            value,
            important,
            line,
            column,
            source_map_column,
        });
        cursor = if segment_end < end {
            segment_end + 1
        } else {
            segment_end
        };
    }
    Ok(())
}

fn find_declaration_end(
    source: &str,
    mut cursor: usize,
    end: usize,
) -> Result<usize, CssParseError> {
    let bytes = source.as_bytes();
    let mut quote = None;
    let mut paren = 0_usize;
    let mut bracket = 0_usize;
    while cursor < end {
        let byte = bytes[cursor];
        if let Some(mark) = quote {
            if byte == b'\\' {
                cursor = cursor.checked_add(2).ok_or(CssParseError::Limit)?;
                continue;
            }
            if byte == mark {
                quote = None;
            }
            cursor += 1;
            continue;
        }
        if byte == b'/' && cursor + 1 < end && bytes[cursor + 1] == b'*' {
            cursor = skip_comment(source, cursor, end)?;
            continue;
        }
        match byte {
            b'\'' | b'"' => quote = Some(byte),
            b'(' => {
                paren += 1;
                if paren > MAX_COMPONENT_DEPTH {
                    return Err(CssParseError::Limit);
                }
            }
            b')' => paren = paren.checked_sub(1).ok_or(CssParseError::Invalid)?,
            b'[' => {
                bracket += 1;
                if bracket > MAX_COMPONENT_DEPTH {
                    return Err(CssParseError::Limit);
                }
            }
            b']' => bracket = bracket.checked_sub(1).ok_or(CssParseError::Invalid)?,
            b';' if paren == 0 && bracket == 0 => return Ok(cursor),
            b'{' | b'}' if paren == 0 && bracket == 0 => {
                return Err(CssParseError::Unsupported);
            }
            _ => {}
        }
        cursor += 1;
    }
    if quote.is_some() || paren != 0 || bracket != 0 {
        Err(CssParseError::Invalid)
    } else {
        Ok(end)
    }
}

fn find_top_level_colon(
    source: &str,
    mut cursor: usize,
    end: usize,
) -> Result<Option<usize>, CssParseError> {
    let bytes = source.as_bytes();
    let mut quote = None;
    let mut paren = 0_usize;
    let mut bracket = 0_usize;
    while cursor < end {
        let byte = bytes[cursor];
        if let Some(mark) = quote {
            if byte == b'\\' {
                cursor = cursor.checked_add(2).ok_or(CssParseError::Limit)?;
                continue;
            }
            if byte == mark {
                quote = None;
            }
            cursor += 1;
            continue;
        }
        if byte == b'/' && cursor + 1 < end && bytes[cursor + 1] == b'*' {
            cursor = skip_comment(source, cursor, end)?;
            continue;
        }
        match byte {
            b'\'' | b'"' => quote = Some(byte),
            b'(' => paren += 1,
            b')' => paren = paren.checked_sub(1).ok_or(CssParseError::Invalid)?,
            b'[' => bracket += 1,
            b']' => bracket = bracket.checked_sub(1).ok_or(CssParseError::Invalid)?,
            b':' if paren == 0 && bracket == 0 => return Ok(Some(cursor)),
            _ => {}
        }
        cursor += 1;
    }
    Ok(None)
}

fn normalize_prelude(value: &str) -> Result<String, CssParseError> {
    let normalized = collapse_css_ws(&strip_comments(value)?)?;
    if normalized.len() > MAX_SELECTOR_BYTES * 2 {
        return Err(CssParseError::Limit);
    }
    Ok(normalized)
}

fn normalize_selector(value: &str) -> Option<String> {
    let stripped = strip_comments(value).ok()?;
    let normalized = collapse_css_ws(&stripped).ok()?;
    if normalized.is_empty()
        || normalized.len() > MAX_SELECTOR_BYTES
        || contains_unquoted_backslash(&normalized)
    {
        return None;
    }
    Some(normalized)
}

fn normalize_property(value: &str) -> Result<String, CssParseError> {
    let stripped = strip_comments(value)?;
    let property = stripped.trim();
    if property.is_empty()
        || property.len() > MAX_PROPERTY_BYTES
        || property
            .chars()
            .any(|ch| ch.is_whitespace() || ch.is_control())
        || !property
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        return Err(CssParseError::Unsupported);
    }
    Ok(property.to_ascii_lowercase())
}

fn normalize_value_and_priority(value: &str) -> Result<(String, bool), CssParseError> {
    let stripped = strip_comments(value)?;
    let priority = top_level_important_index(&stripped)?;
    let (body, important) = match priority {
        Some(index) => (&stripped[..index], true),
        None => (stripped.as_str(), false),
    };
    let normalized = collapse_css_ws(body)?;
    if normalized.len() > MAX_SOURCE_VALUE_BYTES {
        return Err(CssParseError::Limit);
    }
    Ok((normalized, important))
}

fn normalize_css_value(value: &str) -> Option<String> {
    let stripped = strip_comments(value).ok()?;
    let normalized = collapse_css_ws(&stripped).ok()?;
    (!normalized.is_empty() && normalized.len() <= MAX_SOURCE_VALUE_BYTES).then_some(normalized)
}

fn top_level_important_index(value: &str) -> Result<Option<usize>, CssParseError> {
    let bytes = value.as_bytes();
    let mut cursor = 0_usize;
    let mut quote = None;
    let mut paren = 0_usize;
    let mut bracket = 0_usize;
    while cursor < bytes.len() {
        let byte = bytes[cursor];
        if let Some(mark) = quote {
            if byte == b'\\' {
                cursor = cursor.checked_add(2).ok_or(CssParseError::Limit)?;
                continue;
            }
            if byte == mark {
                quote = None;
            }
            cursor += 1;
            continue;
        }
        match byte {
            b'\'' | b'"' => quote = Some(byte),
            b'(' => paren += 1,
            b')' => paren = paren.checked_sub(1).ok_or(CssParseError::Invalid)?,
            b'[' => bracket += 1,
            b']' => bracket = bracket.checked_sub(1).ok_or(CssParseError::Invalid)?,
            b'!' if paren == 0 && bracket == 0 => {
                let rest = &value[cursor + 1..];
                let trimmed = rest.trim_start_matches(|ch: char| ch.is_ascii_whitespace());
                if trimmed
                    .get(.."important".len())
                    .is_some_and(|prefix| prefix.eq_ignore_ascii_case("important"))
                    && trimmed
                        .get("important".len()..)
                        .is_some_and(|suffix| suffix.trim().is_empty())
                {
                    return Ok(Some(cursor));
                }
            }
            _ => {}
        }
        cursor += 1;
    }
    if quote.is_some() || paren != 0 || bracket != 0 {
        Err(CssParseError::Invalid)
    } else {
        Ok(None)
    }
}

fn strip_comments(value: &str) -> Result<String, CssParseError> {
    let bytes = value.as_bytes();
    let mut output = String::with_capacity(value.len());
    let mut cursor = 0_usize;
    let mut quote: Option<char> = None;
    let mut escaped = false;
    while cursor < bytes.len() {
        if quote.is_none()
            && bytes[cursor] == b'/'
            && cursor + 1 < bytes.len()
            && bytes[cursor + 1] == b'*'
        {
            cursor = skip_comment(value, cursor, bytes.len())?;
            if !output.chars().last().is_some_and(char::is_whitespace) {
                output.push(' ');
            }
            continue;
        }

        let ch = value[cursor..]
            .chars()
            .next()
            .ok_or(CssParseError::Invalid)?;
        output.push(ch);
        cursor += ch.len_utf8();

        if let Some(mark) = quote {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == mark {
                quote = None;
            }
        } else if matches!(ch, '\'' | '"') {
            quote = Some(ch);
        }
    }
    if quote.is_some() || escaped {
        Err(CssParseError::Invalid)
    } else {
        Ok(output)
    }
}

fn collapse_css_ws(value: &str) -> Result<String, CssParseError> {
    let mut output = String::with_capacity(value.len());
    let mut quote = None;
    let mut pending_ws = false;
    let mut chars = value.chars().peekable();
    while let Some(ch) = chars.next() {
        if let Some(mark) = quote {
            output.push(ch);
            if ch == '\\' {
                if let Some(next) = chars.next() {
                    output.push(next);
                }
            } else if ch == mark {
                quote = None;
            }
            continue;
        }
        if matches!(ch, '\'' | '"') {
            if pending_ws && !output.is_empty() {
                output.push(' ');
            }
            pending_ws = false;
            quote = Some(ch);
            output.push(ch);
        } else if ch.is_ascii_whitespace() {
            pending_ws = true;
        } else {
            if pending_ws && !output.is_empty() {
                output.push(' ');
            }
            pending_ws = false;
            output.push(ch);
        }
    }
    if quote.is_some() {
        return Err(CssParseError::Invalid);
    }
    Ok(output.trim().to_owned())
}

fn contains_unquoted_backslash(value: &str) -> bool {
    let mut quote = None;
    let mut escaped = false;
    for ch in value.chars() {
        if let Some(mark) = quote {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == mark {
                quote = None;
            }
            continue;
        }
        if matches!(ch, '\'' | '"') {
            quote = Some(ch);
        } else if ch == '\\' {
            return true;
        }
    }
    false
}

fn first_non_ws_comment(
    source: &str,
    mut cursor: usize,
    end: usize,
) -> Result<Option<usize>, CssParseError> {
    let bytes = source.as_bytes();
    while cursor < end {
        if bytes[cursor].is_ascii_whitespace() {
            cursor += 1;
            continue;
        }
        if bytes[cursor] == b'/' && cursor + 1 < end && bytes[cursor + 1] == b'*' {
            cursor = skip_comment(source, cursor, end)?;
            continue;
        }
        return Ok(Some(cursor));
    }
    Ok(None)
}

fn skip_ws_comments(source: &str, cursor: &mut usize, end: usize) -> Result<(), CssParseError> {
    let bytes = source.as_bytes();
    while *cursor < end {
        if bytes[*cursor].is_ascii_whitespace() {
            *cursor += 1;
            continue;
        }
        if bytes[*cursor] == b'/' && *cursor + 1 < end && bytes[*cursor + 1] == b'*' {
            *cursor = skip_comment(source, *cursor, end)?;
            continue;
        }
        break;
    }
    Ok(())
}

fn skip_comment(source: &str, start: usize, end: usize) -> Result<usize, CssParseError> {
    let bytes = source.as_bytes();
    let mut cursor = start + 2;
    while cursor + 1 < end {
        if bytes[cursor] == b'*' && bytes[cursor + 1] == b'/' {
            return Ok(cursor + 2);
        }
        cursor += 1;
    }
    Err(CssParseError::Invalid)
}

fn at_rule_name(prelude: &str) -> Option<&str> {
    let rest = prelude.strip_prefix('@')?;
    let end = rest
        .find(|ch: char| !(ch.is_ascii_alphanumeric() || ch == '-'))
        .unwrap_or(rest.len());
    (end > 0).then_some(&rest[..end])
}

fn line_columns(source: &str, offset: usize) -> Result<(u32, u32, u32), CssParseError> {
    let prefix = source.get(..offset).ok_or(CssParseError::Invalid)?;
    let mut line = 1_u32;
    let mut scalar_column = 0_u32;
    let mut utf16_column = 0_u32;
    let mut chars = prefix.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch == '\r' {
            if chars.peek() == Some(&'\n') {
                chars.next();
            }
            line = line.checked_add(1).ok_or(CssParseError::Limit)?;
            scalar_column = 0;
            utf16_column = 0;
        } else if ch == '\n' {
            line = line.checked_add(1).ok_or(CssParseError::Limit)?;
            scalar_column = 0;
            utf16_column = 0;
        } else {
            scalar_column = scalar_column.checked_add(1).ok_or(CssParseError::Limit)?;
            utf16_column = utf16_column
                .checked_add(u32::try_from(ch.len_utf16()).map_err(|_| CssParseError::Limit)?)
                .ok_or(CssParseError::Limit)?;
        }
    }

    Ok((line, scalar_column, utf16_column))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn locate(
        source: &str,
        selector: &str,
        property: &str,
        value: &str,
        important: bool,
    ) -> Vec<ParsedDeclaration> {
        let selector = normalize_selector(selector).expect("selector");
        let value = normalize_css_value(value).expect("value");
        parse_stylesheet(source)
            .expect("parse CSS")
            .into_iter()
            .filter(|candidate| {
                candidate.selector == selector
                    && candidate.property == property
                    && candidate.value == value
                    && candidate.important == important
            })
            .collect()
    }

    #[test]
    fn simple_exact_match_and_minified_position_are_deterministic() {
        let matches = locate(".save{color:red}", ".save", "color", "red", false);
        assert_eq!(matches.len(), 1);
        assert_eq!((matches[0].line, matches[0].column), (1, 6));
    }

    #[test]
    fn multiple_declarations_and_duplicate_selectors_remain_ambiguous() {
        let source = ".save { color: red; color: red; }\n.save { color: red; }";
        assert_eq!(locate(source, ".save", "color", "red", false).len(), 3);
    }

    #[test]
    fn comments_quoted_semicolons_and_data_urls_do_not_split_declarations() {
        let source = r#"
            /* before */
            .save {
                content: "a;b";
                background-image: url("data:image/svg+xml;utf8,<svg></svg>");
                color: /* x */ red !important;
            }
        "#;
        let color = locate(source, ".save", "color", "red", true);
        assert_eq!(color.len(), 1);
        assert_eq!(
            locate(source, ".save", "content", r#""a;b""#, false).len(),
            1
        );
        assert_eq!(
            locate(
                source,
                ".save",
                "background-image",
                r#"url("data:image/svg+xml;utf8,<svg></svg>")"#,
                false,
            )
            .len(),
            1
        );
    }

    #[test]
    fn nested_media_and_supports_are_located_without_claiming_runtime_activity() {
        let source =
            "@media (min-width: 1px) { @supports (display: grid) { .save { color: red; } } }";
        assert_eq!(locate(source, ".save", "color", "red", false).len(), 1);
    }

    #[test]
    fn unsupported_cascade_containers_fail_closed() {
        assert_eq!(
            parse_stylesheet("@layer base { .save { color: red; } }"),
            Err(CssParseError::Unsupported)
        );
        assert_eq!(
            parse_stylesheet(".save { & .child { color: red; } }"),
            Err(CssParseError::Unsupported)
        );
    }

    #[test]
    fn crlf_and_utf8_columns_are_counted_from_source_text() {
        let crlf = ".x { color: blue; }\r\n.save { color: red; }";
        let match_crlf = &locate(crlf, ".save", "color", "red", false)[0];
        assert_eq!((match_crlf.line, match_crlf.column), (2, 8));

        let utf8 = ".é{color:red}";
        let match_utf8 = &locate(utf8, ".é", "color", "red", false)[0];
        assert_eq!((match_utf8.line, match_utf8.column), (1, 3));
        assert_eq!(match_utf8.source_map_column, 3);
    }

    #[test]
    fn important_text_inside_string_is_not_priority() {
        let source = r#".x { content: "!important"; color: red ! IMPORTANT; }"#;
        assert_eq!(
            locate(source, ".x", "content", r#""!important""#, false).len(),
            1
        );
        assert_eq!(locate(source, ".x", "color", "red", true).len(), 1);
    }

    #[test]
    fn traversal_and_encoded_traversal_are_not_project_relative() {
        assert!(bounded_project_relative_path("../outside.css").is_none());
        assert!(bounded_project_relative_path("src/%2e%2e/outside.css").is_none());
        assert!(bounded_project_relative_path("/tmp/outside.css").is_none());
    }
}
