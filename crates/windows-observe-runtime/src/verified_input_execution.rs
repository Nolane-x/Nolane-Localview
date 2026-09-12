use localview_protocol::DispatchResult;
use localview_windows_uia_provider::{
    WindowsInputInsertionClass, WindowsVerifiedInputBoundaryReceipt,
};

/// Project a provider insertion receipt into the existing durable consequential
/// dispatch states. This says only what the platform insertion boundary did;
/// `DispatchedFull` is not world/postcondition success.
pub fn dispatch_result_for_verified_input(
    receipt: &WindowsVerifiedInputBoundaryReceipt,
) -> DispatchResult {
    match receipt.insertion_class {
        WindowsInputInsertionClass::FullyInserted => DispatchResult::DispatchedFull,
        WindowsInputInsertionClass::PartialDispatchUnknownOutcome => {
            DispatchResult::DispatchedPartial
        }
        WindowsInputInsertionClass::ZeroInsertedBlocked => DispatchResult::NotDispatched,
    }
}
