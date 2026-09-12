use std::convert::Infallible;

use localview_live_bridge::{ConsequentialPostconditionEvidence, ConsequentialPostconditionStatus};
use localview_native_provider::NativeSemanticSnapshotRevision;
use localview_postcondition_contracts::PostconditionContractRegistry;
pub use localview_postcondition_contracts::{
    NativeSemanticNodeMatcherV1, NativeSemanticPostconditionContractError,
    NativeSemanticPostconditionContractV1, NativeSemanticPostconditionEvaluation,
    NativeSemanticPostconditionExpectation,
};
use uuid::Uuid;

use crate::WindowsUiaPostconditionVerifier;

/// Windows UIA adapter from provider-neutral postcondition evaluation into the
/// consequential journal's evidence vocabulary.
///
/// Contract schema ownership lives in `localview-postcondition-contracts`; this
/// runtime contributes only the exact immutable semantic snapshot and a
/// Windows-specific evidence receipt namespace. Unknown or unsupported contract
/// refs remain fail-closed as `Unknown`.
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
        let registry = PostconditionContractRegistry::standard();
        Ok(expected_contract_refs
            .iter()
            .enumerate()
            .map(|(index, contract_ref)| {
                let status = match registry.evaluate_native_semantic(contract_ref, snapshot) {
                    Ok(NativeSemanticPostconditionEvaluation::VerifiedPass) => {
                        ConsequentialPostconditionStatus::VerifiedPass
                    }
                    Ok(NativeSemanticPostconditionEvaluation::VerifiedFail) => {
                        ConsequentialPostconditionStatus::VerifiedFail
                    }
                    Ok(NativeSemanticPostconditionEvaluation::Unknown) | Err(_) => {
                        ConsequentialPostconditionStatus::Unknown
                    }
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
