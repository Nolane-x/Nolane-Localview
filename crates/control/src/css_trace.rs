use std::collections::BTreeMap;

#[path = "css_source_coordinate.rs"]
mod css_source_coordinate;

use axum::{
    Json, Router,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    routing::get,
};
use localview_protocol::SessionId;
use serde::Serialize;
use serde_json::Value;

use crate::{
    ControlState,
    fresh_snapshot::{FreshSnapshotError, acquire_fresh_snapshot_result},
};
use css_source_coordinate::CssSourceAuthority;

const MAX_REFERENCE_BYTES: usize = 256;
const MAX_TREE_DEPTH: usize = 12;
const MAX_SEMANTIC_NODES: usize = 600;
const MAX_COMPUTED_ENTRIES: usize = 48;
const MAX_CSS_PROPERTY_BYTES: usize = 64;
const MAX_CSS_VALUE_BYTES: usize = 256;
const MAX_CSS_DECLARATIONS: usize = 12;
const MAX_CSS_SOURCE_KIND_BYTES: usize = 32;
const MAX_CSS_SOURCE_FILE_BYTES: usize = 260;
const MAX_CSS_SELECTOR_BYTES: usize = 256;
const MAX_CSS_CASCADE_WINNERS: usize = 8;
const MAX_CSS_UNRESOLVED_PROPERTIES: usize = 8;
const MAX_CSS_SOURCE_ORDER: u64 = 512;
const MAX_CSS_SPECIFICITY_UNIT: u64 = 255;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub(crate) struct CssDeclarationEvidence {
    pub source_kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stylesheet_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selector: Option<String>,
    pub property: String,
    pub value: String,
    pub important: bool,
    pub source_authority: CssSourceAuthority,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub(crate) struct CssCascadeWinner {
    pub source_kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stylesheet_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selector: Option<String>,
    pub property: String,
    pub value: String,
    pub important: bool,
    pub specificity: [u16; 4],
    pub source_order: u32,
    pub source_authority: CssSourceAuthority,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub(crate) struct CssAuthorCascade {
    pub scope: String,
    pub coverage_complete: bool,
    pub unresolved_properties: Vec<String>,
    pub winners: Vec<CssCascadeWinner>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub(crate) struct CssStyleTrace {
    pub reference: String,
    pub computed: BTreeMap<String, String>,
    pub declarations: Vec<CssDeclarationEvidence>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub author_cascade: Option<CssAuthorCascade>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CssTraceError {
    InvalidReference,
    InvalidSnapshot,
    ReferenceNotFound,
    AmbiguousReference,
    TraceUnavailable,
}

pub(crate) fn router(state: ControlState) -> Router {
    Router::new()
        .route(
            "/v1/sessions/{id}/style-trace/{reference}",
            get(session_style_trace),
        )
        .with_state(state)
}

async fn session_style_trace(
    State(state): State<ControlState>,
    headers: HeaderMap,
    Path((id, reference)): Path<(SessionId, String)>,
) -> axum::response::Response {
    if !authorized(&headers, &state) {
        return bounded_error(StatusCode::UNAUTHORIZED, "unauthorized");
    }
    if !valid_reference(&reference) {
        return bounded_error(StatusCode::BAD_REQUEST, "invalid_element_reference");
    }

    let result = match acquire_fresh_snapshot_result(&state, id).await {
        Ok(result) => result,
        Err(FreshSnapshotError::SessionNotFound) => {
            return bounded_error(StatusCode::NOT_FOUND, "session_not_found");
        }
        Err(FreshSnapshotError::Timeout) => {
            return bounded_error(StatusCode::GATEWAY_TIMEOUT, "fresh_style_trace_timeout");
        }
        Err(FreshSnapshotError::Failed) => {
            return bounded_error(StatusCode::BAD_GATEWAY, "fresh_style_trace_failed");
        }
        Err(FreshSnapshotError::Invalid) => {
            return bounded_error(StatusCode::BAD_GATEWAY, "invalid_fresh_style_trace");
        }
    };

    match project_style_trace(&result.payload, &reference) {
        Ok(mut trace) => {
            css_source_coordinate::enrich_style_trace_source_authority(&state, id, &mut trace).await;
            Json(trace).into_response()
        }
        Err(CssTraceError::InvalidReference) => {
            bounded_error(StatusCode::BAD_REQUEST, "invalid_element_reference")
        }
        Err(CssTraceError::ReferenceNotFound) => {
            bounded_error(StatusCode::NOT_FOUND, "element_reference_not_found")
        }
        Err(CssTraceError::AmbiguousReference) => {
            bounded_error(StatusCode::CONFLICT, "ambiguous_element_reference")
        }
        Err(CssTraceError::TraceUnavailable) => {
            bounded_error(StatusCode::CONFLICT, "style_trace_unavailable")
        }
        Err(CssTraceError::InvalidSnapshot) => {
            bounded_error(StatusCode::BAD_GATEWAY, "invalid_fresh_style_trace")
        }
    }
}

fn project_style_trace(payload: &Value, reference: &str) -> Result<CssStyleTrace, CssTraceError> {
    if !valid_reference(reference) {
        return Err(CssTraceError::InvalidReference);
    }
    let root = payload
        .get("semantic_tree")
        .filter(|value| !value.is_null())
        .ok_or(CssTraceError::InvalidSnapshot)?;

    let mut remaining = MAX_SEMANTIC_NODES;
    let mut found = None;
    find_node(root, reference, 0, &mut remaining, &mut found)?;
    let node = found.ok_or(CssTraceError::ReferenceNotFound)?;

    let computed = project_computed(node.get("style"))?;
    let declarations = project_declarations(node.get("styleTrace"))?;
    let author_cascade = project_author_cascade(node.get("styleTrace"))?;
    if computed.is_empty() && declarations.is_empty() && author_cascade.is_none() {
        return Err(CssTraceError::TraceUnavailable);
    }

    Ok(CssStyleTrace {
        reference: reference.to_owned(),
        computed,
        declarations,
        author_cascade,
    })
}

fn find_node<'a>(
    node: &'a Value,
    reference: &str,
    depth: usize,
    remaining: &mut usize,
    found: &mut Option<&'a Value>,
) -> Result<(), CssTraceError> {
    if depth > MAX_TREE_DEPTH || *remaining == 0 {
        return Err(CssTraceError::InvalidSnapshot);
    }
    *remaining -= 1;

    let object = node.as_object().ok_or(CssTraceError::InvalidSnapshot)?;
    let node_ref = bounded_string(object.get("ref"), MAX_REFERENCE_BYTES)
        .ok_or(CssTraceError::InvalidSnapshot)?;
    if node_ref == reference {
        if found.is_some() {
            return Err(CssTraceError::AmbiguousReference);
        }
        *found = Some(node);
    }

    let children = object
        .get("children")
        .and_then(Value::as_array)
        .ok_or(CssTraceError::InvalidSnapshot)?;
    for child in children {
        find_node(child, reference, depth + 1, remaining, found)?;
    }
    Ok(())
}

fn project_computed(value: Option<&Value>) -> Result<BTreeMap<String, String>, CssTraceError> {
    let Some(value) = value else {
        return Ok(BTreeMap::new());
    };
    if value.is_null() {
        return Ok(BTreeMap::new());
    }
    let map = value.as_object().ok_or(CssTraceError::InvalidSnapshot)?;
    if map.len() > MAX_COMPUTED_ENTRIES {
        return Err(CssTraceError::InvalidSnapshot);
    }

    let mut output = BTreeMap::new();
    for (property, value) in map {
        if !valid_css_property(property) {
            return Err(CssTraceError::InvalidSnapshot);
        }
        let value = bounded_string(Some(value), MAX_CSS_VALUE_BYTES)
            .ok_or(CssTraceError::InvalidSnapshot)?;
        output.insert(property.clone(), value.to_owned());
    }
    Ok(output)
}

fn project_declarations(
    value: Option<&Value>,
) -> Result<Vec<CssDeclarationEvidence>, CssTraceError> {
    let Some(value) = value else {
        return Ok(Vec::new());
    };
    if value.is_null() {
        return Ok(Vec::new());
    }
    let declarations = value
        .get("declarations")
        .and_then(Value::as_array)
        .ok_or(CssTraceError::InvalidSnapshot)?;
    if declarations.len() > MAX_CSS_DECLARATIONS {
        return Err(CssTraceError::InvalidSnapshot);
    }

    declarations.iter().map(project_declaration).collect()
}

fn project_author_cascade(
    value: Option<&Value>,
) -> Result<Option<CssAuthorCascade>, CssTraceError> {
    let Some(style_trace) = value else {
        return Ok(None);
    };
    if style_trace.is_null() {
        return Ok(None);
    }
    let Some(cascade) = style_trace.get("authorCascade") else {
        return Ok(None);
    };
    if cascade.is_null() {
        return Ok(None);
    }

    let object = cascade.as_object().ok_or(CssTraceError::InvalidSnapshot)?;
    let scope = bounded_string(object.get("scope"), 64).ok_or(CssTraceError::InvalidSnapshot)?;
    if scope != "supported_author_subset" {
        return Err(CssTraceError::InvalidSnapshot);
    }
    let coverage_complete = object
        .get("coverage_complete")
        .and_then(Value::as_bool)
        .ok_or(CssTraceError::InvalidSnapshot)?;

    let unresolved_values = object
        .get("unresolved_properties")
        .and_then(Value::as_array)
        .ok_or(CssTraceError::InvalidSnapshot)?;
    if unresolved_values.len() > MAX_CSS_UNRESOLVED_PROPERTIES {
        return Err(CssTraceError::InvalidSnapshot);
    }
    let mut unresolved_properties = Vec::with_capacity(unresolved_values.len());
    let mut unresolved_seen = std::collections::BTreeSet::new();
    for property in unresolved_values {
        let property = bounded_string(Some(property), MAX_CSS_PROPERTY_BYTES)
            .ok_or(CssTraceError::InvalidSnapshot)?;
        if !valid_css_author_cascade_property(property)
            || !unresolved_seen.insert(property.to_owned())
        {
            return Err(CssTraceError::InvalidSnapshot);
        }
        unresolved_properties.push(property.to_owned());
    }

    let winner_values = object
        .get("winners")
        .and_then(Value::as_array)
        .ok_or(CssTraceError::InvalidSnapshot)?;
    if winner_values.len() > MAX_CSS_CASCADE_WINNERS
        || (!coverage_complete && !winner_values.is_empty())
    {
        return Err(CssTraceError::InvalidSnapshot);
    }

    let mut winners = Vec::with_capacity(winner_values.len());
    let mut winner_seen = std::collections::BTreeSet::new();
    for value in winner_values {
        let winner = project_cascade_winner(value)?;
        if unresolved_seen.contains(&winner.property)
            || !winner_seen.insert(winner.property.clone())
        {
            return Err(CssTraceError::InvalidSnapshot);
        }
        winners.push(winner);
    }

    Ok(Some(CssAuthorCascade {
        scope: scope.to_owned(),
        coverage_complete,
        unresolved_properties,
        winners,
    }))
}

fn project_cascade_winner(value: &Value) -> Result<CssCascadeWinner, CssTraceError> {
    let object = value.as_object().ok_or(CssTraceError::InvalidSnapshot)?;
    validate_runtime_relevance_markers(object)?;
    let source_kind = bounded_string(object.get("source_kind"), MAX_CSS_SOURCE_KIND_BYTES)
        .ok_or(CssTraceError::InvalidSnapshot)?;
    if !matches!(
        source_kind,
        "inline_element" | "same_origin_stylesheet" | "inline_stylesheet"
    ) {
        return Err(CssTraceError::InvalidSnapshot);
    }

    let stylesheet_path =
        optional_bounded_string(object.get("stylesheet_path"), MAX_CSS_SOURCE_FILE_BYTES)?;
    if stylesheet_path
        .as_deref()
        .is_some_and(|path| !valid_relative_file(path))
    {
        return Err(CssTraceError::InvalidSnapshot);
    }

    let selector = optional_bounded_string(object.get("selector"), MAX_CSS_SELECTOR_BYTES)?;
    let property = bounded_string(object.get("property"), MAX_CSS_PROPERTY_BYTES)
        .ok_or(CssTraceError::InvalidSnapshot)?;
    if !valid_css_author_cascade_property(property) {
        return Err(CssTraceError::InvalidSnapshot);
    }
    let value = bounded_string(object.get("value"), MAX_CSS_VALUE_BYTES)
        .ok_or(CssTraceError::InvalidSnapshot)?
        .to_owned();
    let important = object
        .get("important")
        .and_then(Value::as_bool)
        .ok_or(CssTraceError::InvalidSnapshot)?;

    let specificity_values = object
        .get("specificity")
        .and_then(Value::as_array)
        .ok_or(CssTraceError::InvalidSnapshot)?;
    if specificity_values.len() != 4 {
        return Err(CssTraceError::InvalidSnapshot);
    }
    let mut specificity = [0_u16; 4];
    for (index, raw) in specificity_values.iter().enumerate() {
        let value = raw.as_u64().ok_or(CssTraceError::InvalidSnapshot)?;
        if value > MAX_CSS_SPECIFICITY_UNIT {
            return Err(CssTraceError::InvalidSnapshot);
        }
        specificity[index] = u16::try_from(value).map_err(|_| CssTraceError::InvalidSnapshot)?;
    }

    let source_order = object
        .get("source_order")
        .and_then(Value::as_u64)
        .ok_or(CssTraceError::InvalidSnapshot)?;
    if source_order > MAX_CSS_SOURCE_ORDER {
        return Err(CssTraceError::InvalidSnapshot);
    }
    let source_order = u32::try_from(source_order).map_err(|_| CssTraceError::InvalidSnapshot)?;

    match source_kind {
        "inline_element" => {
            if stylesheet_path.is_some()
                || selector.is_some()
                || specificity != [1, 0, 0, 0]
                || source_order != 0
            {
                return Err(CssTraceError::InvalidSnapshot);
            }
        }
        "same_origin_stylesheet" => {
            if stylesheet_path.is_none()
                || selector.is_none()
                || specificity[0] != 0
                || source_order == 0
            {
                return Err(CssTraceError::InvalidSnapshot);
            }
        }
        "inline_stylesheet" => {
            if stylesheet_path.is_some()
                || selector.is_none()
                || specificity[0] != 0
                || source_order == 0
            {
                return Err(CssTraceError::InvalidSnapshot);
            }
        }
        _ => unreachable!("source kind validated above"),
    }

    Ok(CssCascadeWinner {
        source_kind: source_kind.to_owned(),
        stylesheet_path,
        selector,
        property: property.to_owned(),
        value,
        important,
        specificity,
        source_order,
        source_authority: CssSourceAuthority::initial(source_kind),
    })
}

fn project_declaration(value: &Value) -> Result<CssDeclarationEvidence, CssTraceError> {
    let object = value.as_object().ok_or(CssTraceError::InvalidSnapshot)?;
    validate_runtime_relevance_markers(object)?;
    let source_kind = bounded_string(object.get("source_kind"), MAX_CSS_SOURCE_KIND_BYTES)
        .ok_or(CssTraceError::InvalidSnapshot)?;
    if !matches!(
        source_kind,
        "inline_element" | "same_origin_stylesheet" | "inline_stylesheet"
    ) {
        return Err(CssTraceError::InvalidSnapshot);
    }

    let file = optional_bounded_string(object.get("stylesheet_path"), MAX_CSS_SOURCE_FILE_BYTES)?;
    if file
        .as_deref()
        .is_some_and(|file| !valid_relative_file(file))
    {
        return Err(CssTraceError::InvalidSnapshot);
    }
    if source_kind == "same_origin_stylesheet" && file.is_none() {
        return Err(CssTraceError::InvalidSnapshot);
    }
    if source_kind != "same_origin_stylesheet" && file.is_some() {
        return Err(CssTraceError::InvalidSnapshot);
    }

    let selector = optional_bounded_string(object.get("selector"), MAX_CSS_SELECTOR_BYTES)?;
    if source_kind == "inline_element" && selector.is_some() {
        return Err(CssTraceError::InvalidSnapshot);
    }
    if source_kind != "inline_element" && selector.is_none() {
        return Err(CssTraceError::InvalidSnapshot);
    }

    let property = bounded_string(object.get("property"), MAX_CSS_PROPERTY_BYTES)
        .ok_or(CssTraceError::InvalidSnapshot)?;
    if !valid_css_property(property) {
        return Err(CssTraceError::InvalidSnapshot);
    }
    let value = bounded_string(object.get("value"), MAX_CSS_VALUE_BYTES)
        .ok_or(CssTraceError::InvalidSnapshot)?
        .to_owned();
    let important = object
        .get("important")
        .and_then(Value::as_bool)
        .ok_or(CssTraceError::InvalidSnapshot)?;

    Ok(CssDeclarationEvidence {
        source_kind: source_kind.to_owned(),
        stylesheet_path: file,
        selector,
        property: property.to_owned(),
        value,
        important,
        source_authority: CssSourceAuthority::initial(source_kind),
    })
}

fn validate_runtime_relevance_markers(
    object: &serde_json::Map<String, Value>,
) -> Result<(), CssTraceError> {
    if let Some(active) = object.get("active") {
        if active.as_bool() != Some(true) {
            return Err(CssTraceError::InvalidSnapshot);
        }
    }
    if let Some(disabled) = object.get("disabled") {
        if disabled.as_bool() != Some(false) {
            return Err(CssTraceError::InvalidSnapshot);
        }
    }
    Ok(())
}

fn valid_css_author_cascade_property(property: &str) -> bool {
    matches!(
        property,
        "display"
            | "position"
            | "box-sizing"
            | "z-index"
            | "opacity"
            | "pointer-events"
            | "visibility"
            | "color"
    )
}

fn valid_css_property(property: &str) -> bool {
    matches!(
        property,
        "display"
            | "position"
            | "overflowX"
            | "overflowY"
            | "boxSizing"
            | "zIndex"
            | "flexDirection"
            | "flexWrap"
            | "justifyContent"
            | "alignItems"
            | "gap"
            | "rowGap"
            | "columnGap"
            | "gridTemplateColumns"
            | "gridTemplateRows"
            | "paddingTop"
            | "paddingRight"
            | "paddingBottom"
            | "paddingLeft"
            | "marginTop"
            | "marginRight"
            | "marginBottom"
            | "marginLeft"
            | "borderTopWidth"
            | "borderRightWidth"
            | "borderBottomWidth"
            | "borderLeftWidth"
            | "fontSize"
            | "fontWeight"
            | "fontFamily"
            | "lineHeight"
            | "color"
            | "backgroundColor"
            | "opacity"
            | "pointerEvents"
            | "visibility"
            | "overflow-x"
            | "overflow-y"
            | "box-sizing"
            | "z-index"
            | "flex-direction"
            | "flex-wrap"
            | "justify-content"
            | "align-items"
            | "row-gap"
            | "column-gap"
            | "grid-template-columns"
            | "grid-template-rows"
            | "padding-top"
            | "padding-right"
            | "padding-bottom"
            | "padding-left"
            | "margin-top"
            | "margin-right"
            | "margin-bottom"
            | "margin-left"
            | "border-top-width"
            | "border-right-width"
            | "border-bottom-width"
            | "border-left-width"
            | "font-size"
            | "font-weight"
            | "font-family"
            | "line-height"
            | "background-color"
            | "pointer-events"
    )
}

fn valid_relative_file(file: &str) -> bool {
    if file.is_empty()
        || file.starts_with('/')
        || file.contains('\\')
        || file
            .chars()
            .any(|character| matches!(character, '%' | '?' | '#' | ':') || character.is_control())
    {
        return false;
    }
    file.split('/')
        .all(|segment| !segment.is_empty() && segment != "." && segment != "..")
}

fn valid_reference(reference: &str) -> bool {
    !reference.is_empty()
        && reference.len() <= MAX_REFERENCE_BYTES
        && !reference.chars().any(char::is_control)
}

fn bounded_string(value: Option<&Value>, max_bytes: usize) -> Option<&str> {
    let value = value?.as_str()?;
    if value.is_empty() || value.len() > max_bytes || value.chars().any(char::is_control) {
        return None;
    }
    Some(value)
}

fn optional_bounded_string(
    value: Option<&Value>,
    max_bytes: usize,
) -> Result<Option<String>, CssTraceError> {
    let Some(value) = value else {
        return Ok(None);
    };
    if value.is_null() {
        return Ok(None);
    }
    bounded_string(Some(value), max_bytes)
        .map(|value| Some(value.to_owned()))
        .ok_or(CssTraceError::InvalidSnapshot)
}

fn authorized(headers: &HeaderMap, state: &ControlState) -> bool {
    let expected = format!("Bearer {}", state.token);
    headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value == expected)
}

fn bounded_error(status: StatusCode, code: &'static str) -> axum::response::Response {
    (status, Json(serde_json::json!({"error": code}))).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn payload(node: Value) -> Value {
        serde_json::json!({
            "semantic_tree": node
        })
    }

    #[test]
    fn projects_bounded_same_origin_css_declaration_evidence() {
        let trace = project_style_trace(
            &payload(serde_json::json!({
                "ref": "@save",
                "style": {
                    "display": "flex",
                    "paddingTop": "16px"
                },
                "styleTrace": {
                    "declarations": [{
                        "source_kind": "same_origin_stylesheet",
                        "stylesheet_path": "src/button.css",
                        "selector": ".save",
                        "property": "padding-top",
                        "value": "16px",
                        "important": false
                    }]
                },
                "children": []
            })),
            "@save",
        )
        .expect("bounded CSS trace");

        assert_eq!(trace.reference, "@save");
        assert_eq!(
            trace.computed.get("paddingTop").map(String::as_str),
            Some("16px")
        );
        assert_eq!(trace.declarations.len(), 1);
        assert_eq!(
            trace.declarations[0].stylesheet_path.as_deref(),
            Some("src/button.css")
        );
    }

    #[test]
    fn rejects_untrusted_css_file_identity_and_duplicate_refs() {
        let unsafe_file = project_style_trace(
            &payload(serde_json::json!({
                "ref": "@save",
                "style": {"display": "block"},
                "styleTrace": {
                    "declarations": [{
                        "source_kind": "same_origin_stylesheet",
                        "stylesheet_path": "../private.css",
                        "selector": ".save",
                        "property": "display",
                        "value": "block",
                        "important": false
                    }]
                },
                "children": []
            })),
            "@save",
        );
        assert_eq!(unsafe_file, Err(CssTraceError::InvalidSnapshot));

        let duplicate = project_style_trace(
            &payload(serde_json::json!({
                "ref": "@root",
                "style": null,
                "styleTrace": null,
                "children": [
                    {"ref": "@same", "style": {"display": "block"}, "styleTrace": {"declarations": []}, "children": []},
                    {"ref": "@same", "style": {"display": "flex"}, "styleTrace": {"declarations": []}, "children": []}
                ]
            })),
            "@same",
        );
        assert_eq!(duplicate, Err(CssTraceError::AmbiguousReference));
    }

    #[test]
    fn projects_bounded_author_cascade_winner_evidence() {
        let trace = project_style_trace(
            &payload(serde_json::json!({
                "ref": "@save",
                "style": {"display": "flex", "color": "rgb(20, 20, 20)"},
                "styleTrace": {
                    "declarations": [],
                    "authorCascade": {
                        "scope": "supported_author_subset",
                        "coverage_complete": true,
                        "unresolved_properties": ["color"],
                        "winners": [{
                            "source_kind": "same_origin_stylesheet",
                            "stylesheet_path": "src/button.css",
                            "selector": "#app .save",
                            "property": "display",
                            "value": "flex",
                            "important": false,
                            "specificity": [0, 1, 1, 0],
                            "source_order": 7
                        }]
                    }
                },
                "children": []
            })),
            "@save",
        )
        .expect("bounded author cascade");

        let cascade = trace.author_cascade.expect("author cascade");
        assert!(cascade.coverage_complete);
        assert_eq!(cascade.scope, "supported_author_subset");
        assert_eq!(cascade.unresolved_properties, vec!["color"]);
        assert_eq!(cascade.winners.len(), 1);
        assert_eq!(cascade.winners[0].property, "display");
        assert_eq!(cascade.winners[0].specificity, [0, 1, 1, 0]);
        assert_eq!(cascade.winners[0].source_order, 7);
    }

    #[test]
    fn rejects_false_or_conflicting_author_cascade_proof() {
        let incomplete_with_winner = project_style_trace(
            &payload(serde_json::json!({
                "ref": "@save",
                "style": {"display": "flex"},
                "styleTrace": {
                    "declarations": [],
                    "authorCascade": {
                        "scope": "supported_author_subset",
                        "coverage_complete": false,
                        "unresolved_properties": [],
                        "winners": [{
                            "source_kind": "inline_element",
                            "property": "display",
                            "value": "flex",
                            "important": false,
                            "specificity": [1, 0, 0, 0],
                            "source_order": 0
                        }]
                    }
                },
                "children": []
            })),
            "@save",
        );
        assert_eq!(incomplete_with_winner, Err(CssTraceError::InvalidSnapshot));

        let unresolved_winner = project_style_trace(
            &payload(serde_json::json!({
                "ref": "@save",
                "style": {"display": "flex"},
                "styleTrace": {
                    "declarations": [],
                    "authorCascade": {
                        "scope": "supported_author_subset",
                        "coverage_complete": true,
                        "unresolved_properties": ["display"],
                        "winners": [{
                            "source_kind": "inline_element",
                            "property": "display",
                            "value": "flex",
                            "important": false,
                            "specificity": [1, 0, 0, 0],
                            "source_order": 0
                        }]
                    }
                },
                "children": []
            })),
            "@save",
        );
        assert_eq!(unresolved_winner, Err(CssTraceError::InvalidSnapshot));

        let bad_inline_specificity = project_style_trace(
            &payload(serde_json::json!({
                "ref": "@save",
                "style": {"display": "flex"},
                "styleTrace": {
                    "declarations": [],
                    "authorCascade": {
                        "scope": "supported_author_subset",
                        "coverage_complete": true,
                        "unresolved_properties": [],
                        "winners": [{
                            "source_kind": "inline_element",
                            "property": "display",
                            "value": "flex",
                            "important": false,
                            "specificity": [0, 1, 0, 0],
                            "source_order": 0
                        }]
                    }
                },
                "children": []
            })),
            "@save",
        );
        assert_eq!(bad_inline_specificity, Err(CssTraceError::InvalidSnapshot));
    }

    #[test]
    fn rejects_remote_encoded_traversal_and_explicitly_inactive_css_evidence() {
        for declaration in [
            serde_json::json!({
                "source_kind": "same_origin_stylesheet",
                "stylesheet_path": "src/%2e%2e/private.css",
                "selector": ".save",
                "property": "color",
                "value": "red",
                "important": false
            }),
            serde_json::json!({
                "source_kind": "cross_origin_stylesheet",
                "stylesheet_path": "remote.css",
                "selector": ".save",
                "property": "color",
                "value": "red",
                "important": false
            }),
            serde_json::json!({
                "source_kind": "same_origin_stylesheet",
                "stylesheet_path": "src/button.css",
                "selector": ".save",
                "property": "color",
                "value": "red",
                "important": false,
                "active": false
            }),
            serde_json::json!({
                "source_kind": "same_origin_stylesheet",
                "stylesheet_path": "src/button.css",
                "selector": ".save",
                "property": "color",
                "value": "red",
                "important": false,
                "disabled": true
            }),
        ] {
            let result = project_style_trace(
                &payload(serde_json::json!({
                    "ref": "@save",
                    "style": {"color": "red"},
                    "styleTrace": {"declarations": [declaration]},
                    "children": []
                })),
                "@save",
            );
            assert_eq!(result, Err(CssTraceError::InvalidSnapshot));
        }
    }

}
