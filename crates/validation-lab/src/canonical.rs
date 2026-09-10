use std::collections::BTreeMap;
use std::fmt::Write as _;

use serde::Serialize;
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};

use crate::LabError;

const CANONICAL_DOMAIN: &[u8] = b"localview-validation-lab/v1\0";

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, serde::Deserialize)]
pub struct CanonicalDigest(pub String);

pub fn canonical_json_bytes<T: Serialize>(value: &T) -> Result<Vec<u8>, LabError> {
    let value = serde_json::to_value(value).map_err(|error| LabError::CanonicalSerialization {
        message: error.to_string(),
    })?;
    let normalized = normalize(value);
    serde_json::to_vec(&normalized).map_err(|error| LabError::CanonicalSerialization {
        message: error.to_string(),
    })
}

pub fn canonical_digest<T: Serialize>(value: &T) -> Result<CanonicalDigest, LabError> {
    let canonical_bytes = canonical_json_bytes(value)?;
    Ok(digest_canonical_bytes(&canonical_bytes))
}

pub(crate) fn digest_canonical_bytes(canonical_bytes: &[u8]) -> CanonicalDigest {
    let mut hasher = Sha256::new();
    hasher.update(CANONICAL_DOMAIN);
    hasher.update(canonical_bytes);
    let digest = hasher.finalize();

    let mut encoded = String::with_capacity(64);
    for byte in digest {
        write!(&mut encoded, "{byte:02x}").expect("writing to a String cannot fail");
    }
    CanonicalDigest(encoded)
}

fn normalize(value: Value) -> Value {
    match value {
        Value::Object(map) => {
            let sorted = map
                .into_iter()
                .map(|(key, value)| (key, normalize(value)))
                .collect::<BTreeMap<_, _>>();
            let mut normalized = Map::new();
            for (key, value) in sorted {
                normalized.insert(key, value);
            }
            Value::Object(normalized)
        }
        Value::Array(values) => Value::Array(values.into_iter().map(normalize).collect()),
        other => other,
    }
}
