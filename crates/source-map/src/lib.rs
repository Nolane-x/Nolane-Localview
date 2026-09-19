#![forbid(unsafe_code)]

use localview_protocol::SourceLocation;
use regex::Regex;
use serde::{Deserialize, Serialize};

const MAX_JSON_BYTES: usize = 2 * 1024 * 1024;
const MAX_MAPPINGS_BYTES: usize = 1024 * 1024;
const MAX_SOURCES: usize = 4_096;
const MAX_NAMES: usize = 8_192;
const MAX_STRING_BYTES: usize = 1_024;
const MAX_SEGMENTS: usize = 250_000;
const MAX_GENERATED_LINES: usize = 1_000_000;
const MAX_COLUMN: i64 = 10_000_000;
const MAX_ORIGINAL_LINE_ZERO_BASED: i64 = 9_999_999;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SourceHint {
    pub component: Option<String>,
    pub file: Option<String>,
    pub line: Option<u32>,
    pub confidence: u8,
    pub origin: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ResolvedSourceLocation {
    pub source: String,
    pub line: u32,
    pub column: u32,
    pub name: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SourceMapError {
    JsonTooLarge,
    InvalidJson,
    UnsupportedVersion(u32),
    IndexedMapUnsupported,
    TooManySources,
    TooManyNames,
    StringTooLong,
    MappingsTooLarge,
    TooManyGeneratedLines,
    TooManySegments,
    EmptySegment,
    InvalidBase64,
    TruncatedVlq,
    IntegerOverflow,
    InvalidSegmentFieldCount(usize),
    GeneratedColumnOutOfRange,
    GeneratedColumnNotMonotonic,
    SourceIndexOutOfRange,
    OriginalLineOutOfRange,
    OriginalColumnOutOfRange,
    NameIndexOutOfRange,
}

#[derive(Debug, Clone)]
struct MappingSegment {
    generated_column: u32,
    original: Option<ResolvedSourceLocation>,
}

#[derive(Debug, Clone)]
pub struct SourceMap {
    lines: Vec<Vec<MappingSegment>>,
}

#[derive(Debug, Deserialize)]
struct RawSourceMap {
    version: u32,
    #[serde(default, rename = "sourceRoot")]
    source_root: Option<String>,
    #[serde(default)]
    sources: Vec<String>,
    #[serde(default)]
    names: Vec<String>,
    #[serde(default)]
    mappings: String,
    #[serde(default)]
    sections: Option<serde_json::Value>,
}

impl SourceMap {
    pub fn parse(json: &str) -> Result<Self, SourceMapError> {
        if json.len() > MAX_JSON_BYTES {
            return Err(SourceMapError::JsonTooLarge);
        }

        let raw: RawSourceMap =
            serde_json::from_str(json).map_err(|_| SourceMapError::InvalidJson)?;

        if raw.version != 3 {
            return Err(SourceMapError::UnsupportedVersion(raw.version));
        }
        if raw.sections.is_some() {
            return Err(SourceMapError::IndexedMapUnsupported);
        }
        if raw.sources.len() > MAX_SOURCES {
            return Err(SourceMapError::TooManySources);
        }
        if raw.names.len() > MAX_NAMES {
            return Err(SourceMapError::TooManyNames);
        }
        if raw.mappings.len() > MAX_MAPPINGS_BYTES {
            return Err(SourceMapError::MappingsTooLarge);
        }
        if raw
            .source_root
            .as_deref()
            .is_some_and(|value| value.len() > MAX_STRING_BYTES)
            || raw.sources.iter().any(|value| value.len() > MAX_STRING_BYTES)
            || raw.names.iter().any(|value| value.len() > MAX_STRING_BYTES)
        {
            return Err(SourceMapError::StringTooLong);
        }

        let sources = raw
            .sources
            .iter()
            .map(|source| normalize_source_reference(raw.source_root.as_deref(), source))
            .collect::<Vec<_>>();
        if sources.iter().any(|source| source.len() > MAX_STRING_BYTES) {
            return Err(SourceMapError::StringTooLong);
        }

        let mut previous_source = 0_i64;
        let mut previous_original_line = 0_i64;
        let mut previous_original_column = 0_i64;
        let mut previous_name = 0_i64;
        let mut decoded_segments = 0_usize;
        let mut lines = Vec::new();

        for encoded_line in raw.mappings.split(';') {
            if lines.len() >= MAX_GENERATED_LINES {
                return Err(SourceMapError::TooManyGeneratedLines);
            }

            let mut generated_column = 0_i64;
            let mut line_segments = Vec::new();

            if !encoded_line.is_empty() {
                for encoded_segment in encoded_line.split(',') {
                    if encoded_segment.is_empty() {
                        return Err(SourceMapError::EmptySegment);
                    }
                    decoded_segments = decoded_segments
                        .checked_add(1)
                        .ok_or(SourceMapError::IntegerOverflow)?;
                    if decoded_segments > MAX_SEGMENTS {
                        return Err(SourceMapError::TooManySegments);
                    }

                    let fields = decode_vlq_segment(encoded_segment)?;
                    if !matches!(fields.len(), 1 | 4 | 5) {
                        return Err(SourceMapError::InvalidSegmentFieldCount(fields.len()));
                    }

                    let previous_generated_column = generated_column;
                    generated_column = generated_column
                        .checked_add(fields[0])
                        .ok_or(SourceMapError::IntegerOverflow)?;
                    if !(0..=MAX_COLUMN).contains(&generated_column) {
                        return Err(SourceMapError::GeneratedColumnOutOfRange);
                    }
                    if generated_column < previous_generated_column {
                        return Err(SourceMapError::GeneratedColumnNotMonotonic);
                    }

                    let original = if fields.len() == 1 {
                        None
                    } else {
                        previous_source = previous_source
                            .checked_add(fields[1])
                            .ok_or(SourceMapError::IntegerOverflow)?;
                        previous_original_line = previous_original_line
                            .checked_add(fields[2])
                            .ok_or(SourceMapError::IntegerOverflow)?;
                        previous_original_column = previous_original_column
                            .checked_add(fields[3])
                            .ok_or(SourceMapError::IntegerOverflow)?;

                        if previous_source < 0
                            || usize::try_from(previous_source)
                                .ok()
                                .is_none_or(|index| index >= sources.len())
                        {
                            return Err(SourceMapError::SourceIndexOutOfRange);
                        }
                        if !(0..=MAX_ORIGINAL_LINE_ZERO_BASED).contains(&previous_original_line) {
                            return Err(SourceMapError::OriginalLineOutOfRange);
                        }
                        if !(0..=MAX_COLUMN).contains(&previous_original_column) {
                            return Err(SourceMapError::OriginalColumnOutOfRange);
                        }

                        let name = if fields.len() == 5 {
                            previous_name = previous_name
                                .checked_add(fields[4])
                                .ok_or(SourceMapError::IntegerOverflow)?;
                            if previous_name < 0
                                || usize::try_from(previous_name)
                                    .ok()
                                    .is_none_or(|index| index >= raw.names.len())
                            {
                                return Err(SourceMapError::NameIndexOutOfRange);
                            }
                            Some(raw.names[previous_name as usize].clone())
                        } else {
                            None
                        };

                        Some(ResolvedSourceLocation {
                            source: sources[previous_source as usize].clone(),
                            line: u32::try_from(previous_original_line + 1)
                                .map_err(|_| SourceMapError::OriginalLineOutOfRange)?,
                            column: u32::try_from(previous_original_column)
                                .map_err(|_| SourceMapError::OriginalColumnOutOfRange)?,
                            name,
                        })
                    };

                    line_segments.push(MappingSegment {
                        generated_column: u32::try_from(generated_column)
                            .map_err(|_| SourceMapError::GeneratedColumnOutOfRange)?,
                        original,
                    });
                }
            }

            lines.push(line_segments);
        }

        Ok(Self { lines })
    }

    pub fn resolve(
        &self,
        generated_line: u32,
        generated_column: u32,
    ) -> Option<ResolvedSourceLocation> {
        let line_index = usize::try_from(generated_line.checked_sub(1)?).ok()?;
        let segments = self.lines.get(line_index)?;

        let segment = segments
            .iter()
            .take_while(|segment| segment.generated_column <= generated_column)
            .last()?;

        segment.original.clone()
    }

    pub fn generated_line_count(&self) -> usize {
        self.lines.len()
    }
}

pub fn parse_stack_locations(stack: &str) -> Vec<SourceLocation> {
    let re = Regex::new(
        r"(?m)(?:\(|\s|^)([^\s()]+\.(?:tsx?|jsx?|vue|svelte)):(\d+):(\d+)\)?",
    )
    .expect("static stack location regex");

    re.captures_iter(stack)
        .filter_map(|captures| {
            Some(SourceLocation {
                file: captures.get(1)?.as_str().to_owned(),
                line: captures.get(2)?.as_str().parse().ok()?,
                column: captures
                    .get(3)
                    .and_then(|value| value.as_str().parse().ok()),
                component: None,
            })
        })
        .collect()
}

pub fn rank_hints(
    component: Option<&str>,
    stack: &str,
    attributes: &[(String, String)],
) -> Vec<SourceHint> {
    let mut hints = parse_stack_locations(stack)
        .into_iter()
        .map(|source| SourceHint {
            component: component.map(str::to_owned),
            file: Some(source.file),
            line: Some(source.line),
            confidence: 90,
            origin: "stack".into(),
        })
        .collect::<Vec<_>>();

    for (key, value) in attributes {
        if key == "data-source" || key == "data-component-source" {
            let (mut file, mut line) = (value.as_str(), None);
            if let Some(index) = value.rfind(':') {
                if let Ok(parsed_line) = value[index + 1..].parse() {
                    file = &value[..index];
                    line = Some(parsed_line);
                }
            }
            hints.push(SourceHint {
                component: component.map(str::to_owned),
                file: Some(file.to_owned()),
                line,
                confidence: 100,
                origin: key.clone(),
            });
        }
    }

    hints.sort_by_key(|hint| std::cmp::Reverse(hint.confidence));
    hints
}

fn decode_vlq_segment(segment: &str) -> Result<Vec<i64>, SourceMapError> {
    let bytes = segment.as_bytes();
    let mut cursor = 0_usize;
    let mut values = Vec::new();

    while cursor < bytes.len() {
        let mut accumulated = 0_u64;
        let mut shift = 0_u32;

        loop {
            let byte = *bytes.get(cursor).ok_or(SourceMapError::TruncatedVlq)?;
            cursor += 1;
            let digit = base64_value(byte).ok_or(SourceMapError::InvalidBase64)?;
            let payload = u64::from(digit & 0b1_1111);
            let shifted = payload
                .checked_shl(shift)
                .ok_or(SourceMapError::IntegerOverflow)?;
            accumulated = accumulated
                .checked_add(shifted)
                .ok_or(SourceMapError::IntegerOverflow)?;

            let continuation = digit & 0b10_0000 != 0;
            if !continuation {
                break;
            }
            shift = shift.checked_add(5).ok_or(SourceMapError::IntegerOverflow)?;
            if shift >= 64 || cursor >= bytes.len() {
                return Err(SourceMapError::TruncatedVlq);
            }
        }

        let negative = accumulated & 1 == 1;
        let magnitude = accumulated >> 1;
        let magnitude =
            i64::try_from(magnitude).map_err(|_| SourceMapError::IntegerOverflow)?;
        values.push(if negative { -magnitude } else { magnitude });
    }

    Ok(values)
}

fn base64_value(byte: u8) -> Option<u8> {
    match byte {
        b'A'..=b'Z' => Some(byte - b'A'),
        b'a'..=b'z' => Some(byte - b'a' + 26),
        b'0'..=b'9' => Some(byte - b'0' + 52),
        b'+' => Some(62),
        b'/' => Some(63),
        _ => None,
    }
}

fn normalize_source_reference(source_root: Option<&str>, source: &str) -> String {
    let source = strip_query_and_fragment(&source.replace('\\', "/"));
    let combined = if is_absolute_reference(&source) {
        source
    } else if let Some(root) = source_root.filter(|root| !root.is_empty()) {
        format!(
            "{}/{}",
            strip_query_and_fragment(&root.replace('\\', "/")).trim_end_matches('/'),
            source.trim_start_matches('/')
        )
    } else {
        source
    };

    collapse_dot_components(&combined)
}

fn strip_query_and_fragment(value: &str) -> String {
    let query = value.find('?');
    let fragment = value.find('#');
    let end = match (query, fragment) {
        (Some(query), Some(fragment)) => query.min(fragment),
        (Some(query), None) => query,
        (None, Some(fragment)) => fragment,
        (None, None) => value.len(),
    };
    value[..end].to_owned()
}

fn is_absolute_reference(value: &str) -> bool {
    if value.starts_with('/') {
        return true;
    }

    let bytes = value.as_bytes();
    if bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && bytes[2] == b'/'
    {
        return true;
    }

    if let Some(colon) = value.find(':') {
        let scheme = &value[..colon];
        !scheme.is_empty()
            && scheme
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'+' | b'-' | b'.'))
            && scheme.as_bytes()[0].is_ascii_alphabetic()
    } else {
        false
    }
}

fn collapse_dot_components(value: &str) -> String {
    let (prefix, rest) = if let Some(index) = value.find("://") {
        (&value[..index + 3], &value[index + 3..])
    } else {
        ("", value)
    };

    let leading_slash = prefix.is_empty() && rest.starts_with('/');
    let trailing_slash = rest.ends_with('/') && rest.len() > 1;
    let mut components = Vec::new();

    for component in rest.split('/') {
        if component.is_empty() || component == "." {
            continue;
        }
        components.push(component);
    }

    let mut normalized = String::new();
    normalized.push_str(prefix);
    if leading_slash {
        normalized.push('/');
    }
    normalized.push_str(&components.join("/"));
    if trailing_slash && !normalized.ends_with('/') {
        normalized.push('/');
    }
    normalized
}

#[cfg(test)]
mod tests {
    use super::*;

    fn map_json(mappings: &str) -> String {
        serde_json::json!({
            "version": 3,
            "sourceRoot": "src/./",
            "sources": ["components/./Button.tsx", "pages/App.tsx"],
            "names": ["render"],
            "sourcesContent": ["MUST-NOT-BE-RETAINED", "MUST-NOT-BE-RETAINED"],
            "mappings": mappings
        })
        .to_string()
    }

    #[test]
    fn parses_vite_stack() {
        let locations = parse_stack_locations("at save (src/components/Button.tsx:84:12)");
        assert_eq!(locations[0].line, 84);
        assert_eq!(locations[0].column, Some(12));
    }

    #[test]
    fn vlq_decodes_zero_positive_negative_and_continuation() {
        assert_eq!(decode_vlq_segment("A").unwrap(), vec![0]);
        assert_eq!(decode_vlq_segment("C").unwrap(), vec![1]);
        assert_eq!(decode_vlq_segment("D").unwrap(), vec![-1]);
        assert_eq!(decode_vlq_segment("gB").unwrap(), vec![16]);
    }

    #[test]
    fn vlq_rejects_invalid_and_truncated_input() {
        assert_eq!(
            decode_vlq_segment("!").unwrap_err(),
            SourceMapError::InvalidBase64
        );
        assert_eq!(
            decode_vlq_segment("g").unwrap_err(),
            SourceMapError::TruncatedVlq
        );
    }

    #[test]
    fn resolves_generated_positions_with_delta_state_across_lines() {
        let map = SourceMap::parse(&map_json("AAAA,UACEA;ACEE")).unwrap();

        assert_eq!(
            map.resolve(1, 0),
            Some(ResolvedSourceLocation {
                source: "src/components/Button.tsx".into(),
                line: 1,
                column: 0,
                name: None,
            })
        );
        assert_eq!(
            map.resolve(1, 10),
            Some(ResolvedSourceLocation {
                source: "src/components/Button.tsx".into(),
                line: 2,
                column: 2,
                name: Some("render".into()),
            })
        );
        assert_eq!(
            map.resolve(2, 0),
            Some(ResolvedSourceLocation {
                source: "src/pages/App.tsx".into(),
                line: 4,
                column: 4,
                name: None,
            })
        );
        assert_eq!(map.generated_line_count(), 2);
    }

    #[test]
    fn unmapped_segment_fails_closed_until_another_mapping_exists() {
        let map = SourceMap::parse(&map_json("AAAA,K")).unwrap();

        assert!(map.resolve(1, 4).is_some());
        assert_eq!(map.resolve(1, 5), None);
        assert_eq!(map.resolve(1, 99), None);
    }

    #[test]
    fn lookup_never_falls_back_to_previous_generated_line() {
        let map = SourceMap::parse(&map_json("AAAA;;")).unwrap();

        assert!(map.resolve(1, 0).is_some());
        assert_eq!(map.resolve(2, 0), None);
        assert_eq!(map.resolve(3, 0), None);
        assert_eq!(map.resolve(4, 0), None);
    }

    #[test]
    fn source_references_strip_query_and_fragment_metadata() {
        let json = serde_json::json!({
            "version": 3,
            "sourceRoot": "https://localhost/src?token=MUST-NOT-LEAK",
            "sources": ["Button.tsx?secret=ALSO-NOT#fragment"],
            "names": [],
            "mappings": "AAAA"
        })
        .to_string();

        let map = SourceMap::parse(&json).unwrap();
        let resolved = map.resolve(1, 0).unwrap();
        assert_eq!(resolved.source, "https://localhost/src/Button.tsx");
        assert!(!format!("{map:?}").contains("MUST-NOT-LEAK"));
        assert!(!format!("{map:?}").contains("ALSO-NOT"));
    }

    #[test]
    fn normalized_source_reference_remains_hard_bounded() {
        let root = "r".repeat(700);
        let source = "s".repeat(700);
        let json = serde_json::json!({
            "version": 3,
            "sourceRoot": root,
            "sources": [source],
            "names": [],
            "mappings": "AAAA"
        })
        .to_string();

        assert_eq!(
            SourceMap::parse(&json).unwrap_err(),
            SourceMapError::StringTooLong
        );
    }

    #[test]
    fn source_root_and_dot_components_are_normalized_without_erasing_parent_segments() {
        let json = serde_json::json!({
            "version": 3,
            "sourceRoot": "webpack://app/./src",
            "sources": ["../components/./Button.tsx"],
            "names": [],
            "mappings": "AAAA"
        })
        .to_string();

        let map = SourceMap::parse(&json).unwrap();
        assert_eq!(
            map.resolve(1, 0).unwrap().source,
            "webpack://app/src/../components/Button.tsx"
        );
    }

    #[test]
    fn rejects_invalid_version_indexed_maps_and_out_of_range_indices() {
        let invalid_version = serde_json::json!({
            "version": 2,
            "sources": [],
            "names": [],
            "mappings": ""
        })
        .to_string();
        assert_eq!(
            SourceMap::parse(&invalid_version).unwrap_err(),
            SourceMapError::UnsupportedVersion(2)
        );

        let indexed = serde_json::json!({
            "version": 3,
            "sections": []
        })
        .to_string();
        assert_eq!(
            SourceMap::parse(&indexed).unwrap_err(),
            SourceMapError::IndexedMapUnsupported
        );

        let bad_source_index = serde_json::json!({
            "version": 3,
            "sources": [],
            "names": [],
            "mappings": "ACAA"
        })
        .to_string();
        assert_eq!(
            SourceMap::parse(&bad_source_index).unwrap_err(),
            SourceMapError::SourceIndexOutOfRange
        );
    }

    #[test]
    fn rejects_invalid_segment_shapes_and_negative_generated_columns() {
        let invalid_shape = serde_json::json!({
            "version": 3,
            "sources": ["a.ts"],
            "names": [],
            "mappings": "AA"
        })
        .to_string();
        assert_eq!(
            SourceMap::parse(&invalid_shape).unwrap_err(),
            SourceMapError::InvalidSegmentFieldCount(2)
        );

        let negative_generated = serde_json::json!({
            "version": 3,
            "sources": ["a.ts"],
            "names": [],
            "mappings": "D"
        })
        .to_string();
        assert_eq!(
            SourceMap::parse(&negative_generated).unwrap_err(),
            SourceMapError::GeneratedColumnOutOfRange
        );
    }

    #[test]
    fn ignores_sources_content_in_runtime_representation() {
        let map = SourceMap::parse(&map_json("AAAA")).unwrap();
        let debug = format!("{map:?}");
        assert!(!debug.contains("MUST-NOT-BE-RETAINED"));
    }
}
