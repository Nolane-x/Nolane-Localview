#![forbid(unsafe_code)]

use std::collections::BTreeMap;

use localview_native_provider::{
    NativeSemanticNodeObservation, NativeSemanticSnapshotRevision,
};
use localview_protocol::ReconciliationCompleteness;
use serde_json::{Map, Value};
use thiserror::Error;

const CONTRACT_ROOT_PREFIX: &str = "lvpc:";
const NATIVE_SEMANTIC_FAMILY: &str = "native-semantic";
const NATIVE_SEMANTIC_VERSION_V1: &str = "1";
const NATIVE_SEMANTIC_VERSION_V2: &str = "2";
const CONTRACT_PREFIX_V1: &str = "lvpc:native-semantic:v1:";
const CONTRACT_PREFIX_V2: &str = "lvpc:native-semantic:v2:";
const CONTRACT_FAMILY_PREFIX: &str = "lvpc:native-semantic:v";

/// Correctness-bearing schema entry admitted by a registry revision.
///
/// The standard registry is intentionally immutable. Adding support for a new
/// family/version therefore requires a code revision rather than runtime plugin
/// mutation, so durable postcondition semantics cannot silently change beneath
/// already-journaled actions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PostconditionContractSchema {
    pub family: &'static str,
    pub version: &'static str,
}

const STANDARD_SCHEMAS: [PostconditionContractSchema; 2] = [
    PostconditionContractSchema {
        family: NATIVE_SEMANTIC_FAMILY,
        version: NATIVE_SEMANTIC_VERSION_V1,
    },
    PostconditionContractSchema {
        family: NATIVE_SEMANTIC_FAMILY,
        version: NATIVE_SEMANTIC_VERSION_V2,
    },
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RegisteredPostconditionContract {
    NativeSemanticV1(NativeSemanticPostconditionContractV1),
    NativeSemanticV2(NativeSemanticPostconditionContractV2),
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum PostconditionContractRegistryError {
    #[error("postcondition contract reference is not structurally valid")]
    InvalidReference,
    #[error("unsupported postcondition contract family {family}")]
    UnsupportedFamily { family: String },
    #[error("unsupported postcondition contract version {family}:v{version}")]
    UnsupportedVersion { family: String, version: String },
    #[error("registered native semantic postcondition contract is invalid: {0}")]
    NativeSemanticContract(NativeSemanticPostconditionContractError),
}

/// Immutable registry of correctness-bearing postcondition schemas understood by
/// this LocalView build.
///
/// This registry performs schema admission/dispatch only. Observation authority
/// remains with the provider/runtime that produced the immutable snapshot.
#[derive(Debug, Clone, Copy, Default)]
pub struct PostconditionContractRegistry;

impl PostconditionContractRegistry {
    pub const fn standard() -> Self {
        Self
    }

    pub fn schemas(&self) -> &'static [PostconditionContractSchema] {
        &STANDARD_SCHEMAS
    }

    pub fn decode(
        &self,
        contract_ref: &str,
    ) -> Result<RegisteredPostconditionContract, PostconditionContractRegistryError> {
        let (family, version) = parse_registry_header(contract_ref)?;
        let family_registered = self.schemas().iter().any(|schema| schema.family == family);
        if !family_registered {
            return Err(PostconditionContractRegistryError::UnsupportedFamily {
                family: family.to_owned(),
            });
        }
        let version_registered = self
            .schemas()
            .iter()
            .any(|schema| schema.family == family && schema.version == version);
        if !version_registered {
            return Err(PostconditionContractRegistryError::UnsupportedVersion {
                family: family.to_owned(),
                version: version.to_owned(),
            });
        }

        match (family, version) {
            (NATIVE_SEMANTIC_FAMILY, NATIVE_SEMANTIC_VERSION_V1) => {
                NativeSemanticPostconditionContractV1::from_contract_ref(contract_ref)
                    .map(RegisteredPostconditionContract::NativeSemanticV1)
                    .map_err(PostconditionContractRegistryError::NativeSemanticContract)
            }
            (NATIVE_SEMANTIC_FAMILY, NATIVE_SEMANTIC_VERSION_V2) => {
                NativeSemanticPostconditionContractV2::from_contract_ref(contract_ref)
                    .map(RegisteredPostconditionContract::NativeSemanticV2)
                    .map_err(PostconditionContractRegistryError::NativeSemanticContract)
            }
            _ => unreachable!("registered postcondition schema lacks a decoder"),
        }
    }

    pub fn evaluate_native_semantic(
        &self,
        contract_ref: &str,
        snapshot: &NativeSemanticSnapshotRevision,
    ) -> Result<NativeSemanticPostconditionEvaluation, PostconditionContractRegistryError> {
        match self.decode(contract_ref)? {
            RegisteredPostconditionContract::NativeSemanticV1(contract) => {
                Ok(contract.evaluate(snapshot))
            }
            RegisteredPostconditionContract::NativeSemanticV2(contract) => {
                Ok(contract.evaluate(snapshot))
            }
        }
    }
}

fn parse_registry_header(
    contract_ref: &str,
) -> Result<(&str, &str), PostconditionContractRegistryError> {
    let rest = contract_ref
        .strip_prefix(CONTRACT_ROOT_PREFIX)
        .ok_or(PostconditionContractRegistryError::InvalidReference)?;
    let mut parts = rest.splitn(3, ':');
    let family = parts
        .next()
        .filter(|value| !value.is_empty())
        .ok_or(PostconditionContractRegistryError::InvalidReference)?;
    let version_tag = parts
        .next()
        .filter(|value| value.starts_with('v') && value.len() > 1)
        .ok_or(PostconditionContractRegistryError::InvalidReference)?;
    let payload = parts
        .next()
        .filter(|value| !value.is_empty())
        .ok_or(PostconditionContractRegistryError::InvalidReference)?;
    let _ = payload;
    Ok((family, &version_tag[1..]))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NativeSemanticPostconditionExpectation {
    Present,
    Absent,
}

impl NativeSemanticPostconditionExpectation {
    fn as_str(self) -> &'static str {
        match self {
            Self::Present => "present",
            Self::Absent => "absent",
        }
    }

    fn parse(value: &Value) -> Result<Self, NativeSemanticPostconditionContractError> {
        match value.as_str() {
            Some("present") => Ok(Self::Present),
            Some("absent") => Ok(Self::Absent),
            _ => Err(NativeSemanticPostconditionContractError::InvalidPayload),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct NativeSemanticNodeMatcherV1 {
    pub role: Option<String>,
    pub name: Option<String>,
    pub control_type: Option<String>,
    pub automation_id: Option<String>,
    pub class_name: Option<String>,
    pub is_enabled: Option<bool>,
    pub is_offscreen: Option<bool>,
    pub attributes: BTreeMap<String, String>,
}

impl NativeSemanticNodeMatcherV1 {
    fn is_empty(&self) -> bool {
        matcher_is_empty(
            self.role.as_ref(),
            self.name.as_ref(),
            self.control_type.as_ref(),
            self.automation_id.as_ref(),
            self.class_name.as_ref(),
            self.is_enabled,
            self.is_offscreen,
            &self.attributes,
        )
    }

    fn matches(&self, node: &NativeSemanticNodeObservation) -> bool {
        matcher_matches(
            self.role.as_deref(),
            self.name.as_deref(),
            self.control_type.as_deref(),
            self.automation_id.as_deref(),
            self.class_name.as_deref(),
            self.is_enabled,
            self.is_offscreen,
            &self.attributes,
            node,
        )
    }

    fn to_json(&self) -> Value {
        matcher_to_json(
            self.role.as_ref(),
            self.name.as_ref(),
            self.control_type.as_ref(),
            self.automation_id.as_ref(),
            self.class_name.as_ref(),
            self.is_enabled,
            self.is_offscreen,
            &self.attributes,
        )
    }

    fn from_json(value: &Value) -> Result<Self, NativeSemanticPostconditionContractError> {
        let parsed = parse_matcher(value)?;
        let matcher = Self {
            role: parsed.role,
            name: parsed.name,
            control_type: parsed.control_type,
            automation_id: parsed.automation_id,
            class_name: parsed.class_name,
            is_enabled: parsed.is_enabled,
            is_offscreen: parsed.is_offscreen,
            attributes: parsed.attributes,
        };
        if matcher.is_empty() {
            return Err(NativeSemanticPostconditionContractError::EmptyMatcher);
        }
        Ok(matcher)
    }
}

/// V2 intentionally owns a distinct matcher type even while its first revision
/// uses the same exact-match fields as V1. This keeps the V1 wire/schema frozen
/// while allowing future V2-only matcher evolution without reinterpreting old
/// durable contract references.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct NativeSemanticNodeMatcherV2 {
    pub role: Option<String>,
    pub name: Option<String>,
    pub control_type: Option<String>,
    pub automation_id: Option<String>,
    pub class_name: Option<String>,
    pub is_enabled: Option<bool>,
    pub is_offscreen: Option<bool>,
    pub attributes: BTreeMap<String, String>,
}

impl NativeSemanticNodeMatcherV2 {
    fn is_empty(&self) -> bool {
        matcher_is_empty(
            self.role.as_ref(),
            self.name.as_ref(),
            self.control_type.as_ref(),
            self.automation_id.as_ref(),
            self.class_name.as_ref(),
            self.is_enabled,
            self.is_offscreen,
            &self.attributes,
        )
    }

    fn matches(&self, node: &NativeSemanticNodeObservation) -> bool {
        matcher_matches(
            self.role.as_deref(),
            self.name.as_deref(),
            self.control_type.as_deref(),
            self.automation_id.as_deref(),
            self.class_name.as_deref(),
            self.is_enabled,
            self.is_offscreen,
            &self.attributes,
            node,
        )
    }

    fn to_canonical_json(&self) -> Result<String, NativeSemanticPostconditionContractError> {
        if self.is_empty() {
            return Err(NativeSemanticPostconditionContractError::EmptyMatcher);
        }
        let mut fields = Vec::new();
        push_canonical_string_field(&mut fields, "role", self.role.as_ref())?;
        push_canonical_string_field(&mut fields, "name", self.name.as_ref())?;
        push_canonical_string_field(&mut fields, "control_type", self.control_type.as_ref())?;
        push_canonical_string_field(&mut fields, "automation_id", self.automation_id.as_ref())?;
        push_canonical_string_field(&mut fields, "class_name", self.class_name.as_ref())?;
        if let Some(value) = self.is_enabled {
            fields.push(format!("\"is_enabled\":{value}"));
        }
        if let Some(value) = self.is_offscreen {
            fields.push(format!("\"is_offscreen\":{value}"));
        }
        if !self.attributes.is_empty() {
            let attributes = serde_json::to_string(&self.attributes)
                .map_err(|_| NativeSemanticPostconditionContractError::InvalidPayload)?;
            fields.push(format!("\"attributes\":{attributes}"));
        }
        Ok(format!("{{{}}}", fields.join(",")))
    }

    fn from_json(value: &Value) -> Result<Self, NativeSemanticPostconditionContractError> {
        let parsed = parse_matcher(value)?;
        let matcher = Self {
            role: parsed.role,
            name: parsed.name,
            control_type: parsed.control_type,
            automation_id: parsed.automation_id,
            class_name: parsed.class_name,
            is_enabled: parsed.is_enabled,
            is_offscreen: parsed.is_offscreen,
            attributes: parsed.attributes,
        };
        if matcher.is_empty() {
            return Err(NativeSemanticPostconditionContractError::EmptyMatcher);
        }
        Ok(matcher)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeSemanticPostconditionContractV1 {
    pub expectation: NativeSemanticPostconditionExpectation,
    pub matcher: NativeSemanticNodeMatcherV1,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NativeSemanticCountComparisonV2 {
    Equal,
    AtLeast,
    AtMost,
}

impl NativeSemanticCountComparisonV2 {
    fn as_str(self) -> &'static str {
        match self {
            Self::Equal => "equal",
            Self::AtLeast => "at_least",
            Self::AtMost => "at_most",
        }
    }

    fn parse(value: &Value) -> Result<Self, NativeSemanticPostconditionContractError> {
        match value.as_str() {
            Some("equal") => Ok(Self::Equal),
            Some("at_least") => Ok(Self::AtLeast),
            Some("at_most") => Ok(Self::AtMost),
            _ => Err(NativeSemanticPostconditionContractError::InvalidPayload),
        }
    }
}

/// V2 adds an explicit cardinality predicate over an exact semantic matcher.
/// It deliberately does not infer application-specific business success from
/// provider attributes; it only proves a count over a complete reconciled cut.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeSemanticPostconditionContractV2 {
    pub comparison: NativeSemanticCountComparisonV2,
    pub count: u32,
    pub matcher: NativeSemanticNodeMatcherV2,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NativeSemanticPostconditionEvaluation {
    VerifiedPass,
    VerifiedFail,
    Unknown,
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum NativeSemanticPostconditionContractError {
    #[error("unsupported native semantic postcondition contract version {version}")]
    UnsupportedVersion { version: String },
    #[error("native semantic postcondition contract family is unsupported")]
    UnsupportedFamily,
    #[error("native semantic postcondition contract payload is invalid")]
    InvalidPayload,
    #[error("native semantic postcondition contract contains an unknown correctness field")]
    UnknownField,
    #[error("native semantic postcondition matcher cannot be empty")]
    EmptyMatcher,
    #[error("native semantic postcondition reference is not canonically encoded")]
    NonCanonicalReference,
}

impl NativeSemanticPostconditionContractV1 {
    pub fn to_contract_ref(&self) -> Result<String, NativeSemanticPostconditionContractError> {
        if self.matcher.is_empty() {
            return Err(NativeSemanticPostconditionContractError::EmptyMatcher);
        }
        let mut object = Map::new();
        object.insert(
            "expectation".into(),
            Value::String(self.expectation.as_str().into()),
        );
        object.insert("matcher".into(), self.matcher.to_json());
        let payload = serde_json::to_string(&Value::Object(object))
            .map_err(|_| NativeSemanticPostconditionContractError::InvalidPayload)?;
        Ok(format!("{CONTRACT_PREFIX_V1}{payload}"))
    }

    pub fn from_contract_ref(
        contract_ref: &str,
    ) -> Result<Self, NativeSemanticPostconditionContractError> {
        let payload = contract_payload_for_version(contract_ref, CONTRACT_PREFIX_V1)?;
        let value: Value = serde_json::from_str(payload)
            .map_err(|_| NativeSemanticPostconditionContractError::InvalidPayload)?;
        let object = value
            .as_object()
            .ok_or(NativeSemanticPostconditionContractError::InvalidPayload)?;
        if object.len() != 2
            || !object.contains_key("expectation")
            || !object.contains_key("matcher")
        {
            return Err(NativeSemanticPostconditionContractError::UnknownField);
        }

        let contract = Self {
            expectation: NativeSemanticPostconditionExpectation::parse(
                object
                    .get("expectation")
                    .ok_or(NativeSemanticPostconditionContractError::InvalidPayload)?,
            )?,
            matcher: NativeSemanticNodeMatcherV1::from_json(
                object
                    .get("matcher")
                    .ok_or(NativeSemanticPostconditionContractError::InvalidPayload)?,
            )?,
        };
        if contract.to_contract_ref()? != contract_ref {
            return Err(NativeSemanticPostconditionContractError::NonCanonicalReference);
        }
        Ok(contract)
    }

    pub fn evaluate(
        &self,
        snapshot: &NativeSemanticSnapshotRevision,
    ) -> NativeSemanticPostconditionEvaluation {
        if !snapshot_is_complete(snapshot) {
            return NativeSemanticPostconditionEvaluation::Unknown;
        }

        let found = snapshot.nodes().iter().any(|node| self.matcher.matches(node));
        match (self.expectation, found) {
            (NativeSemanticPostconditionExpectation::Present, true)
            | (NativeSemanticPostconditionExpectation::Absent, false) => {
                NativeSemanticPostconditionEvaluation::VerifiedPass
            }
            (NativeSemanticPostconditionExpectation::Present, false)
            | (NativeSemanticPostconditionExpectation::Absent, true) => {
                NativeSemanticPostconditionEvaluation::VerifiedFail
            }
        }
    }
}

impl NativeSemanticPostconditionContractV2 {
    pub fn to_contract_ref(&self) -> Result<String, NativeSemanticPostconditionContractError> {
        let matcher = self.matcher.to_canonical_json()?;
        let payload = format!(
            "{{\"comparison\":\"{}\",\"count\":{},\"matcher\":{matcher}}}",
            self.comparison.as_str(),
            self.count
        );
        Ok(format!("{CONTRACT_PREFIX_V2}{payload}"))
    }

    pub fn from_contract_ref(
        contract_ref: &str,
    ) -> Result<Self, NativeSemanticPostconditionContractError> {
        let payload = contract_payload_for_version(contract_ref, CONTRACT_PREFIX_V2)?;
        let value: Value = serde_json::from_str(payload)
            .map_err(|_| NativeSemanticPostconditionContractError::InvalidPayload)?;
        let object = value
            .as_object()
            .ok_or(NativeSemanticPostconditionContractError::InvalidPayload)?;
        if object.len() != 3
            || !object.contains_key("comparison")
            || !object.contains_key("count")
            || !object.contains_key("matcher")
        {
            return Err(NativeSemanticPostconditionContractError::UnknownField);
        }

        let count = object
            .get("count")
            .and_then(Value::as_u64)
            .and_then(|value| u32::try_from(value).ok())
            .ok_or(NativeSemanticPostconditionContractError::InvalidPayload)?;
        let contract = Self {
            comparison: NativeSemanticCountComparisonV2::parse(
                object
                    .get("comparison")
                    .ok_or(NativeSemanticPostconditionContractError::InvalidPayload)?,
            )?,
            count,
            matcher: NativeSemanticNodeMatcherV2::from_json(
                object
                    .get("matcher")
                    .ok_or(NativeSemanticPostconditionContractError::InvalidPayload)?,
            )?,
        };
        if contract.to_contract_ref()? != contract_ref {
            return Err(NativeSemanticPostconditionContractError::NonCanonicalReference);
        }
        Ok(contract)
    }

    pub fn evaluate(
        &self,
        snapshot: &NativeSemanticSnapshotRevision,
    ) -> NativeSemanticPostconditionEvaluation {
        if !snapshot_is_complete(snapshot) {
            return NativeSemanticPostconditionEvaluation::Unknown;
        }

        let observed = snapshot
            .nodes()
            .iter()
            .filter(|node| self.matcher.matches(node))
            .count() as u64;
        let expected = u64::from(self.count);
        let passed = match self.comparison {
            NativeSemanticCountComparisonV2::Equal => observed == expected,
            NativeSemanticCountComparisonV2::AtLeast => observed >= expected,
            NativeSemanticCountComparisonV2::AtMost => observed <= expected,
        };
        if passed {
            NativeSemanticPostconditionEvaluation::VerifiedPass
        } else {
            NativeSemanticPostconditionEvaluation::VerifiedFail
        }
    }
}

#[derive(Debug)]
struct ParsedMatcher {
    role: Option<String>,
    name: Option<String>,
    control_type: Option<String>,
    automation_id: Option<String>,
    class_name: Option<String>,
    is_enabled: Option<bool>,
    is_offscreen: Option<bool>,
    attributes: BTreeMap<String, String>,
}

fn parse_matcher(value: &Value) -> Result<ParsedMatcher, NativeSemanticPostconditionContractError> {
    let object = value
        .as_object()
        .ok_or(NativeSemanticPostconditionContractError::InvalidPayload)?;
    const ALLOWED: &[&str] = &[
        "role",
        "name",
        "control_type",
        "automation_id",
        "class_name",
        "is_enabled",
        "is_offscreen",
        "attributes",
    ];
    if object.keys().any(|key| !ALLOWED.contains(&key.as_str())) {
        return Err(NativeSemanticPostconditionContractError::UnknownField);
    }

    let attributes = match object.get("attributes") {
        None => BTreeMap::new(),
        Some(Value::Object(values)) => values
            .iter()
            .map(|(key, value)| {
                value
                    .as_str()
                    .map(|value| (key.clone(), value.to_owned()))
                    .ok_or(NativeSemanticPostconditionContractError::InvalidPayload)
            })
            .collect::<Result<BTreeMap<_, _>, _>>()?,
        Some(_) => return Err(NativeSemanticPostconditionContractError::InvalidPayload),
    };

    Ok(ParsedMatcher {
        role: optional_string(object.get("role"))?,
        name: optional_string(object.get("name"))?,
        control_type: optional_string(object.get("control_type"))?,
        automation_id: optional_string(object.get("automation_id"))?,
        class_name: optional_string(object.get("class_name"))?,
        is_enabled: optional_bool(object.get("is_enabled"))?,
        is_offscreen: optional_bool(object.get("is_offscreen"))?,
        attributes,
    })
}

fn contract_payload_for_version<'a>(
    contract_ref: &'a str,
    expected_prefix: &str,
) -> Result<&'a str, NativeSemanticPostconditionContractError> {
    if let Some(payload) = contract_ref.strip_prefix(expected_prefix) {
        Ok(payload)
    } else if let Some(rest) = contract_ref.strip_prefix(CONTRACT_FAMILY_PREFIX) {
        let version = rest.split(':').next().unwrap_or(rest).to_owned();
        Err(NativeSemanticPostconditionContractError::UnsupportedVersion { version })
    } else {
        Err(NativeSemanticPostconditionContractError::UnsupportedFamily)
    }
}

fn snapshot_is_complete(snapshot: &NativeSemanticSnapshotRevision) -> bool {
    snapshot.completeness() == ReconciliationCompleteness::Established
        && snapshot.incompleteness_debt().is_empty()
        && !snapshot.resource_usage().incomplete
}

#[allow(clippy::too_many_arguments)]
fn matcher_is_empty(
    role: Option<&String>,
    name: Option<&String>,
    control_type: Option<&String>,
    automation_id: Option<&String>,
    class_name: Option<&String>,
    is_enabled: Option<bool>,
    is_offscreen: Option<bool>,
    attributes: &BTreeMap<String, String>,
) -> bool {
    role.is_none()
        && name.is_none()
        && control_type.is_none()
        && automation_id.is_none()
        && class_name.is_none()
        && is_enabled.is_none()
        && is_offscreen.is_none()
        && attributes.is_empty()
}

#[allow(clippy::too_many_arguments)]
fn matcher_matches(
    role: Option<&str>,
    name: Option<&str>,
    control_type: Option<&str>,
    automation_id: Option<&str>,
    class_name: Option<&str>,
    is_enabled: Option<bool>,
    is_offscreen: Option<bool>,
    attributes: &BTreeMap<String, String>,
    node: &NativeSemanticNodeObservation,
) -> bool {
    option_matches(role, node.role.as_deref())
        && option_matches(name, node.name.as_deref())
        && option_matches(control_type, node.control_type.as_deref())
        && option_matches(automation_id, node.automation_id.as_deref())
        && option_matches(class_name, node.class_name.as_deref())
        && bool_matches(is_enabled, node.is_enabled)
        && bool_matches(is_offscreen, node.is_offscreen)
        && attributes
            .iter()
            .all(|(key, value)| node.attributes.get(key) == Some(value))
}

#[allow(clippy::too_many_arguments)]
fn matcher_to_json(
    role: Option<&String>,
    name: Option<&String>,
    control_type: Option<&String>,
    automation_id: Option<&String>,
    class_name: Option<&String>,
    is_enabled: Option<bool>,
    is_offscreen: Option<bool>,
    attributes: &BTreeMap<String, String>,
) -> Value {
    let mut object = Map::new();
    insert_optional_string(&mut object, "role", role);
    insert_optional_string(&mut object, "name", name);
    insert_optional_string(&mut object, "control_type", control_type);
    insert_optional_string(&mut object, "automation_id", automation_id);
    insert_optional_string(&mut object, "class_name", class_name);
    if let Some(value) = is_enabled {
        object.insert("is_enabled".into(), Value::Bool(value));
    }
    if let Some(value) = is_offscreen {
        object.insert("is_offscreen".into(), Value::Bool(value));
    }
    if !attributes.is_empty() {
        object.insert(
            "attributes".into(),
            Value::Object(
                attributes
                    .iter()
                    .map(|(key, value)| (key.clone(), Value::String(value.clone())))
                    .collect(),
            ),
        );
    }
    Value::Object(object)
}

fn push_canonical_string_field(
    fields: &mut Vec<String>,
    key: &str,
    value: Option<&String>,
) -> Result<(), NativeSemanticPostconditionContractError> {
    if let Some(value) = value {
        let value = serde_json::to_string(value)
            .map_err(|_| NativeSemanticPostconditionContractError::InvalidPayload)?;
        fields.push(format!("\"{key}\":{value}"));
    }
    Ok(())
}

fn option_matches(expected: Option<&str>, observed: Option<&str>) -> bool {
    expected.is_none_or(|expected| observed == Some(expected))
}

fn bool_matches(expected: Option<bool>, observed: Option<bool>) -> bool {
    expected.is_none_or(|expected| observed == Some(expected))
}

fn insert_optional_string(object: &mut Map<String, Value>, key: &str, value: Option<&String>) {
    if let Some(value) = value {
        object.insert(key.into(), Value::String(value.clone()));
    }
}

fn optional_string(
    value: Option<&Value>,
) -> Result<Option<String>, NativeSemanticPostconditionContractError> {
    match value {
        None => Ok(None),
        Some(Value::String(value)) => Ok(Some(value.clone())),
        Some(_) => Err(NativeSemanticPostconditionContractError::InvalidPayload),
    }
}

fn optional_bool(
    value: Option<&Value>,
) -> Result<Option<bool>, NativeSemanticPostconditionContractError> {
    match value {
        None => Ok(None),
        Some(Value::Bool(value)) => Ok(Some(*value)),
        Some(_) => Err(NativeSemanticPostconditionContractError::InvalidPayload),
    }
}
