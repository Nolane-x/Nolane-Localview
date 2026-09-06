use std::{collections::BTreeMap, convert::Infallible};

use localview_live_bridge::{
    ConsequentialPostconditionEvidence, ConsequentialPostconditionStatus,
};
use localview_native_provider::{
    NativeSemanticNodeObservation, NativeSemanticSnapshotRevision,
};
use localview_protocol::ReconciliationCompleteness;
use serde_json::{Map, Value};
use thiserror::Error;
use uuid::Uuid;

use crate::WindowsUiaPostconditionVerifier;

const CONTRACT_PREFIX_V1: &str = "lvpc:native-semantic:v1:";
const CONTRACT_FAMILY_PREFIX: &str = "lvpc:native-semantic:v";

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
        self.role.is_none()
            && self.name.is_none()
            && self.control_type.is_none()
            && self.automation_id.is_none()
            && self.class_name.is_none()
            && self.is_enabled.is_none()
            && self.is_offscreen.is_none()
            && self.attributes.is_empty()
    }

    fn matches(&self, node: &NativeSemanticNodeObservation) -> bool {
        option_matches(self.role.as_deref(), node.role.as_deref())
            && option_matches(self.name.as_deref(), node.name.as_deref())
            && option_matches(self.control_type.as_deref(), node.control_type.as_deref())
            && option_matches(self.automation_id.as_deref(), node.automation_id.as_deref())
            && option_matches(self.class_name.as_deref(), node.class_name.as_deref())
            && bool_matches(self.is_enabled, node.is_enabled)
            && bool_matches(self.is_offscreen, node.is_offscreen)
            && self
                .attributes
                .iter()
                .all(|(key, value)| node.attributes.get(key) == Some(value))
    }

    fn to_json(&self) -> Value {
        let mut object = Map::new();
        insert_optional_string(&mut object, "role", self.role.as_ref());
        insert_optional_string(&mut object, "name", self.name.as_ref());
        insert_optional_string(&mut object, "control_type", self.control_type.as_ref());
        insert_optional_string(&mut object, "automation_id", self.automation_id.as_ref());
        insert_optional_string(&mut object, "class_name", self.class_name.as_ref());
        if let Some(value) = self.is_enabled {
            object.insert("is_enabled".into(), Value::Bool(value));
        }
        if let Some(value) = self.is_offscreen {
            object.insert("is_offscreen".into(), Value::Bool(value));
        }
        if !self.attributes.is_empty() {
            object.insert(
                "attributes".into(),
                Value::Object(
                    self.attributes
                        .iter()
                        .map(|(key, value)| (key.clone(), Value::String(value.clone())))
                        .collect(),
                ),
            );
        }
        Value::Object(object)
    }

    fn from_json(value: &Value) -> Result<Self, NativeSemanticPostconditionContractError> {
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

        let matcher = Self {
            role: optional_string(object.get("role"))?,
            name: optional_string(object.get("name"))?,
            control_type: optional_string(object.get("control_type"))?,
            automation_id: optional_string(object.get("automation_id"))?,
            class_name: optional_string(object.get("class_name"))?,
            is_enabled: optional_bool(object.get("is_enabled"))?,
            is_offscreen: optional_bool(object.get("is_offscreen"))?,
            attributes,
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
        let payload = if let Some(payload) = contract_ref.strip_prefix(CONTRACT_PREFIX_V1) {
            payload
        } else if let Some(rest) = contract_ref.strip_prefix(CONTRACT_FAMILY_PREFIX) {
            let version = rest.split(':').next().unwrap_or(rest).to_owned();
            return Err(NativeSemanticPostconditionContractError::UnsupportedVersion { version });
        } else {
            return Err(NativeSemanticPostconditionContractError::UnsupportedFamily);
        };

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
        if snapshot.completeness() != ReconciliationCompleteness::Established
            || !snapshot.incompleteness_debt().is_empty()
            || snapshot.resource_usage().incomplete
        {
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

#[derive(Debug, Clone, Copy, Default)]
pub struct WindowsUiaSemanticPostconditionVerifier;

impl WindowsUiaPostconditionVerifier for WindowsUiaSemanticPostconditionVerifier {
    type Error = Infallible;

    fn verify(
        &self,
        action_id: Uuid,
        expected_contract_refs: &[String],
        snapshot: &NativeSemanticSnapshotRevision,
    ) -> Result<Vec<ConsequentialPostconditionEvidence>, Self::Error> {
        Ok(expected_contract_refs
            .iter()
            .enumerate()
            .map(|(index, contract_ref)| {
                let status = match NativeSemanticPostconditionContractV1::from_contract_ref(
                    contract_ref,
                ) {
                    Ok(contract) => match contract.evaluate(snapshot) {
                        NativeSemanticPostconditionEvaluation::VerifiedPass => {
                            ConsequentialPostconditionStatus::VerifiedPass
                        }
                        NativeSemanticPostconditionEvaluation::VerifiedFail => {
                            ConsequentialPostconditionStatus::VerifiedFail
                        }
                        NativeSemanticPostconditionEvaluation::Unknown => {
                            ConsequentialPostconditionStatus::Unknown
                        }
                    },
                    Err(_) => ConsequentialPostconditionStatus::Unknown,
                };
                ConsequentialPostconditionEvidence {
                    contract_ref: contract_ref.clone(),
                    status,
                    receipt_ref: format!(
                        "postcondition-evidence:windows-uia:{action_id}:{}:{index}",
                        snapshot.cache_revision_ref()
                    ),
                }
            })
            .collect())
    }
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
