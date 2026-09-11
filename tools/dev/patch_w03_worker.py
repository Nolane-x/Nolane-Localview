from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
worker_path = ROOT / "crates/windows-uia-provider/src/lib.rs"
virtualized_path = ROOT / "crates/windows-uia-provider/src/virtualized_item.rs"


def replace_once(text: str, old: str, new: str, label: str) -> str:
    count = text.count(old)
    if count != 1:
        raise SystemExit(f"{label}: expected exactly one marker, found {count}")
    return text.replace(old, new, 1)


worker = worker_path.read_text(encoding="utf-8")

worker = replace_once(
    worker,
    '    #[error("Windows UI Automation dispatch context is blocked: {0}")]\n',
    '    #[error("Windows UI Automation virtualized-item request does not match worker authority")]\n'
    '    InvalidVirtualizedItemRequest,\n'
    '    #[error("Windows UI Automation ItemContainer pattern is unavailable")]\n'
    '    VirtualizedItemContainerPatternUnavailable,\n'
    '    #[error("Windows UI Automation VirtualizedItem pattern is unavailable")]\n'
    '    VirtualizedItemPatternUnavailable,\n'
    '    #[error("Windows UI Automation retained virtualized placeholder is unavailable")]\n'
    '    VirtualizedItemPlaceholderNotFound,\n'
    '    #[error("Windows UI Automation dispatch context is blocked: {0}")]\n',
    "worker errors",
)

worker = replace_once(
    worker,
    '            Threading::{GetProcessTimes, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION},\n',
    '            Threading::{GetProcessTimes, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION},\n'
    '            Variant::VARIANT,\n',
    "VARIANT import",
)

worker = replace_once(
    worker,
    '                IUIAutomationExpandCollapsePattern, IUIAutomationInvokePattern,\n'
    '                IUIAutomationSelectionItemPattern, IUIAutomationTogglePattern,\n'
    '                IUIAutomationTreeWalker, IUIAutomationValuePattern, UIA_ExpandCollapsePatternId,\n',
    '                IUIAutomationExpandCollapsePattern, IUIAutomationInvokePattern,\n'
    '                IUIAutomationItemContainerPattern, IUIAutomationSelectionItemPattern,\n'
    '                IUIAutomationTogglePattern, IUIAutomationTreeWalker, IUIAutomationValuePattern,\n'
    '                IUIAutomationVirtualizedItemPattern, UIA_AutomationIdPropertyId,\n'
    '                UIA_ExpandCollapsePatternId, UIA_ItemContainerPatternId, UIA_NamePropertyId,\n',
    "UIA interface imports",
)

worker = replace_once(
    worker,
    '                UIA_TogglePatternId, UIA_ToggleToggleStatePropertyId, UIA_ValuePatternId,\n',
    '                UIA_TogglePatternId, UIA_ToggleToggleStatePropertyId, UIA_ValuePatternId,\n'
    '                UIA_VirtualizedItemPatternId,\n',
    "UIA pattern id import",
)

worker = replace_once(
    worker,
    '        WindowsUiaValueCapabilityFacts, evaluate_windows_uia_dispatch_context,\n',
    '        WindowsUiaItemLookupProperty, WindowsUiaValueCapabilityFacts,\n'
    '        WindowsUiaVirtualizedItemQueryReceipt, WindowsUiaVirtualizedItemQueryRequest,\n'
    '        WindowsUiaVirtualizedItemRealizeReceipt, WindowsUiaVirtualizedItemRealizeRequest,\n'
    '        evaluate_windows_uia_dispatch_context,\n',
    "virtualized type imports",
)

worker = replace_once(
    worker,
    '        VerifySetValue {\n'
    '            attachment: WindowsUiaAttachment,\n'
    '            request: WindowsUiaSetValueVerificationRequest,\n'
    '            reply: Sender<Result<WindowsUiaSetValueVerificationReceipt, WindowsUiaWorkerError>>,\n'
    '        },\n'
    '        Shutdown,\n',
    '        VerifySetValue {\n'
    '            attachment: WindowsUiaAttachment,\n'
    '            request: WindowsUiaSetValueVerificationRequest,\n'
    '            reply: Sender<Result<WindowsUiaSetValueVerificationReceipt, WindowsUiaWorkerError>>,\n'
    '        },\n'
    '        QueryVirtualizedItem {\n'
    '            attachment: WindowsUiaAttachment,\n'
    '            request: WindowsUiaVirtualizedItemQueryRequest,\n'
    '            reply: Sender<Result<WindowsUiaVirtualizedItemQueryReceipt, WindowsUiaWorkerError>>,\n'
    '        },\n'
    '        RealizeVirtualizedItem {\n'
    '            attachment: WindowsUiaAttachment,\n'
    '            request: WindowsUiaVirtualizedItemRealizeRequest,\n'
    '            reply: Sender<Result<WindowsUiaVirtualizedItemRealizeReceipt, WindowsUiaWorkerError>>,\n'
    '        },\n'
    '        Shutdown,\n',
    "worker commands",
)

worker = replace_once(
    worker,
    '    struct WorkerState {\n',
    '    struct RetainedVirtualizedPlaceholder {\n'
    '        element_ref: ProviderElementRef,\n'
    '        element: IUIAutomationElement,\n'
    '    }\n\n'
    '    struct RetainedVirtualizedPlaceholderSet {\n'
    '        snapshot_cut_ref: String,\n'
    '        placeholders: Vec<RetainedVirtualizedPlaceholder>,\n'
    '    }\n\n'
    '    struct WorkerState {\n',
    "placeholder structs",
)

worker = replace_once(
    worker,
    '        element_leases: HashMap<TargetIncarnationRef, RetainedElementLeaseSet>,\n'
    '    }\n',
    '        element_leases: HashMap<TargetIncarnationRef, RetainedElementLeaseSet>,\n'
    '        virtualized_placeholders:\n'
    '            HashMap<TargetIncarnationRef, RetainedVirtualizedPlaceholderSet>,\n'
    '    }\n',
    "worker state field",
)

worker_methods = r'''

        pub(crate) fn query_virtualized_item_on_mta(
            &self,
            attachment: &WindowsUiaAttachment,
            request: WindowsUiaVirtualizedItemQueryRequest,
        ) -> Result<WindowsUiaVirtualizedItemQueryReceipt, WindowsUiaWorkerError> {
            let container = request.container_element_ref();
            if attachment.provider_incarnation_ref != self.provider_incarnation_ref
                || container.provider_incarnation_ref != self.provider_incarnation_ref
                || container.target_incarnation_ref != attachment.target_incarnation_ref
                || container.acquisition_cut_ref != request.snapshot_cut_ref()
            {
                return Err(WindowsUiaWorkerError::InvalidVirtualizedItemRequest);
            }

            let (reply_tx, reply_rx) = mpsc::channel();
            self.sender
                .send(WorkerCommand::QueryVirtualizedItem {
                    attachment: attachment.clone(),
                    request,
                    reply: reply_tx,
                })
                .map_err(|_| WindowsUiaWorkerError::WorkerUnavailable)?;
            recv_command(reply_rx, self.command_timeout)
        }

        pub(crate) fn realize_virtualized_item_on_mta(
            &self,
            attachment: &WindowsUiaAttachment,
            request: WindowsUiaVirtualizedItemRealizeRequest,
        ) -> Result<WindowsUiaVirtualizedItemRealizeReceipt, WindowsUiaWorkerError> {
            let placeholder = request.placeholder_element_ref();
            if attachment.provider_incarnation_ref != self.provider_incarnation_ref
                || placeholder.provider_incarnation_ref != self.provider_incarnation_ref
                || placeholder.target_incarnation_ref != attachment.target_incarnation_ref
                || placeholder.acquisition_cut_ref != request.snapshot_cut_ref()
            {
                return Err(WindowsUiaWorkerError::InvalidVirtualizedItemRequest);
            }

            let (reply_tx, reply_rx) = mpsc::channel();
            self.sender
                .send(WorkerCommand::RealizeVirtualizedItem {
                    attachment: attachment.clone(),
                    request,
                    reply: reply_tx,
                })
                .map_err(|_| WindowsUiaWorkerError::WorkerUnavailable)?;
            recv_command(reply_rx, self.command_timeout)
        }
'''
worker = replace_once(
    worker,
    '        pub fn verify_set_value(\n',
    worker_methods + '\n        pub fn verify_set_value(\n',
    "worker public internal methods",
)

worker = replace_once(
    worker,
    '            element_leases: HashMap::new(),\n'
    '        };\n',
    '            element_leases: HashMap::new(),\n'
    '            virtualized_placeholders: HashMap::new(),\n'
    '        };\n',
    "worker init",
)

worker = replace_once(
    worker,
    '                WorkerCommand::VerifySetValue {\n'
    '                    attachment,\n'
    '                    request,\n'
    '                    reply,\n'
    '                } => {\n'
    '                    let _ = reply.send(state.verify_set_value(&attachment, request));\n'
    '                }\n'
    '                WorkerCommand::Shutdown => break,\n',
    '                WorkerCommand::VerifySetValue {\n'
    '                    attachment,\n'
    '                    request,\n'
    '                    reply,\n'
    '                } => {\n'
    '                    let _ = reply.send(state.verify_set_value(&attachment, request));\n'
    '                }\n'
    '                WorkerCommand::QueryVirtualizedItem {\n'
    '                    attachment,\n'
    '                    request,\n'
    '                    reply,\n'
    '                } => {\n'
    '                    let _ = reply.send(state.query_virtualized_item(&attachment, request));\n'
    '                }\n'
    '                WorkerCommand::RealizeVirtualizedItem {\n'
    '                    attachment,\n'
    '                    request,\n'
    '                    reply,\n'
    '                } => {\n'
    '                    let _ = reply.send(state.realize_virtualized_item(&attachment, request));\n'
    '                }\n'
    '                WorkerCommand::Shutdown => break,\n',
    "worker dispatch",
)

worker = replace_once(
    worker,
    '            self.element_leases.insert(\n'
    '                attachment.target_incarnation_ref.clone(),\n'
    '                RetainedElementLeaseSet {\n'
    '                    snapshot_cut_ref: revision.snapshot_cut_ref().to_owned(),\n'
    '                    elements: retained_elements,\n'
    '                },\n'
    '            );\n'
    '            Ok(revision)\n',
    '            self.element_leases.insert(\n'
    '                attachment.target_incarnation_ref.clone(),\n'
    '                RetainedElementLeaseSet {\n'
    '                    snapshot_cut_ref: revision.snapshot_cut_ref().to_owned(),\n'
    '                    elements: retained_elements,\n'
    '                },\n'
    '            );\n'
    '            // Any successful observation cut supersedes temporary placeholder\n'
    '            // retention. A realization receipt never promotes the old ref; only\n'
    '            // nodes observed in this new cut may become RealizedCurrent.\n'
    '            self.virtualized_placeholders\n'
    '                .remove(&attachment.target_incarnation_ref);\n'
    '            Ok(revision)\n',
    "snapshot invalidates placeholders",
)

state_methods = r'''

        fn query_virtualized_item(
            &mut self,
            attachment: &WindowsUiaAttachment,
            request: WindowsUiaVirtualizedItemQueryRequest,
        ) -> Result<WindowsUiaVirtualizedItemQueryReceipt, WindowsUiaWorkerError> {
            let container = {
                let retained = self.exact_retained_element(
                    attachment,
                    request.snapshot_cut_ref(),
                    request.container_element_ref(),
                )?;
                retained.element.clone()
            };

            let item_container = unsafe {
                // SAFETY: the retained container and returned pattern stay on this
                // worker's owning MTA for the entire lookup.
                container.GetCurrentPatternAs::<IUIAutomationItemContainerPattern>(
                    UIA_ItemContainerPatternId,
                )
            }
            .map_err(|_| WindowsUiaWorkerError::VirtualizedItemContainerPatternUnavailable)?;

            let property_id = match request.property() {
                WindowsUiaItemLookupProperty::Name => UIA_NamePropertyId,
                WindowsUiaItemLookupProperty::AutomationId => UIA_AutomationIdPropertyId,
            };
            let lookup_value = VARIANT::from(request.value());
            let placeholder = unsafe {
                // SAFETY: start-after is intentionally null to search the entire
                // exact ItemContainer; the VARIANT is local and valid for this call.
                item_container.FindItemByProperty(
                    None::<&IUIAutomationElement>,
                    property_id,
                    &lookup_value,
                )
            }
            .map_err(|error| WindowsUiaWorkerError::ProviderFailure(error.to_string()))?;

            // Microsoft requires a virtualized placeholder to support this pattern;
            // do not probe arbitrary placeholder properties because they may return
            // UIA_E_ELEMENTNOTAVAILABLE before realization.
            unsafe {
                placeholder.GetCurrentPatternAs::<IUIAutomationVirtualizedItemPattern>(
                    UIA_VirtualizedItemPatternId,
                )
            }
            .map_err(|_| WindowsUiaWorkerError::VirtualizedItemPatternUnavailable)?;

            let runtime_id = unsafe {
                // SAFETY: best-effort identity hint read from the MTA-owned placeholder.
                runtime_id_hint(&placeholder)
            }
            .unwrap_or_default();
            let mut placeholder_element_ref = provider_element_ref_from_runtime_id(
                self.provider_incarnation_ref.clone(),
                attachment.target_incarnation_ref.clone(),
                &runtime_id,
                request.snapshot_cut_ref(),
                ProviderElementRealization::RealizationRequired,
            );
            if runtime_id.is_empty() {
                placeholder_element_ref.opaque_provider_element_id =
                    format!("uia-virtualized-placeholder:{}", Uuid::new_v4());
            }
            placeholder_element_ref.parent_surface_ref =
                request.container_element_ref().parent_surface_ref.clone();
            placeholder_element_ref.semantic_locator_hints.push(match request.property() {
                WindowsUiaItemLookupProperty::Name => format!("name={}", request.value()),
                WindowsUiaItemLookupProperty::AutomationId => {
                    format!("automation_id={}", request.value())
                }
            });

            let receipt = WindowsUiaVirtualizedItemQueryReceipt {
                snapshot_cut_ref: request.snapshot_cut_ref().to_owned(),
                provider_incarnation_ref: self.provider_incarnation_ref.clone(),
                target_incarnation_ref: attachment.target_incarnation_ref.clone(),
                container_element_ref: request.container_element_ref().clone(),
                placeholder_element_ref: placeholder_element_ref.clone(),
            };
            self.virtualized_placeholders.insert(
                attachment.target_incarnation_ref.clone(),
                RetainedVirtualizedPlaceholderSet {
                    snapshot_cut_ref: request.snapshot_cut_ref().to_owned(),
                    placeholders: vec![RetainedVirtualizedPlaceholder {
                        element_ref: placeholder_element_ref,
                        element: placeholder,
                    }],
                },
            );
            Ok(receipt)
        }

        fn realize_virtualized_item(
            &mut self,
            attachment: &WindowsUiaAttachment,
            request: WindowsUiaVirtualizedItemRealizeRequest,
        ) -> Result<WindowsUiaVirtualizedItemRealizeReceipt, WindowsUiaWorkerError> {
            self.require_current_target(attachment)?;
            let placeholder = {
                let retained = self
                    .virtualized_placeholders
                    .get(&attachment.target_incarnation_ref)
                    .ok_or(WindowsUiaWorkerError::VirtualizedItemPlaceholderNotFound)?;
                if retained.snapshot_cut_ref != request.snapshot_cut_ref() {
                    return Err(WindowsUiaWorkerError::ElementLeaseSnapshotExpired {
                        requested_cut: request.snapshot_cut_ref().to_owned(),
                        current_cut: retained.snapshot_cut_ref.clone(),
                    });
                }
                retained
                    .placeholders
                    .iter()
                    .find(|retained| {
                        &retained.element_ref == request.placeholder_element_ref()
                    })
                    .ok_or(WindowsUiaWorkerError::VirtualizedItemPlaceholderNotFound)?
                    .element
                    .clone()
            };

            let virtualized_item = unsafe {
                // SAFETY: placeholder and pattern remain on this worker's MTA.
                placeholder.GetCurrentPatternAs::<IUIAutomationVirtualizedItemPattern>(
                    UIA_VirtualizedItemPatternId,
                )
            }
            .map_err(|_| WindowsUiaWorkerError::VirtualizedItemPatternUnavailable)?;
            unsafe {
                // SAFETY: this is the single provider-side realization operation.
                virtualized_item.Realize()
            }
            .map_err(|error| WindowsUiaWorkerError::ProviderFailure(error.to_string()))?;

            self.virtualized_placeholders
                .remove(&attachment.target_incarnation_ref);
            Ok(WindowsUiaVirtualizedItemRealizeReceipt {
                provider_incarnation_ref: self.provider_incarnation_ref.clone(),
                target_incarnation_ref: attachment.target_incarnation_ref.clone(),
                previous_placeholder_ref: request.placeholder_element_ref().clone(),
            })
        }
'''
worker = replace_once(
    worker,
    '        fn revalidate_dispatch_context(\n',
    state_methods + '\n        fn revalidate_dispatch_context(\n',
    "worker state methods",
)

worker_path.write_text(worker, encoding="utf-8")

virtualized = virtualized_path.read_text(encoding="utf-8")
old_query = '''    pub fn query_virtualized_item(\n        &self,\n        _attachment: &crate::worker::WindowsUiaAttachment,\n        _request: WindowsUiaVirtualizedItemQueryRequest,\n    ) -> Result<WindowsUiaVirtualizedItemQueryReceipt, crate::worker::WindowsUiaWorkerError> {\n        Err(crate::worker::WindowsUiaWorkerError::ProviderFailure(\n            "Windows UIA virtualized-item query is not implemented".into(),\n        ))\n    }\n'''
new_query = '''    pub fn query_virtualized_item(\n        &self,\n        attachment: &crate::worker::WindowsUiaAttachment,\n        request: WindowsUiaVirtualizedItemQueryRequest,\n    ) -> Result<WindowsUiaVirtualizedItemQueryReceipt, crate::worker::WindowsUiaWorkerError> {\n        #[cfg(windows)]\n        {\n            self.query_virtualized_item_on_mta(attachment, request)\n        }\n        #[cfg(not(windows))]\n        {\n            let _ = (attachment, request);\n            Err(crate::worker::WindowsUiaWorkerError::UnsupportedPlatform)\n        }\n    }\n'''
virtualized = replace_once(virtualized, old_query, new_query, "public query delegate")

old_realize = '''    pub fn realize_virtualized_item(\n        &self,\n        _attachment: &crate::worker::WindowsUiaAttachment,\n        _request: WindowsUiaVirtualizedItemRealizeRequest,\n    ) -> Result<WindowsUiaVirtualizedItemRealizeReceipt, crate::worker::WindowsUiaWorkerError> {\n        Err(crate::worker::WindowsUiaWorkerError::ProviderFailure(\n            "Windows UIA virtualized-item realization is not implemented".into(),\n        ))\n    }\n'''
new_realize = '''    pub fn realize_virtualized_item(\n        &self,\n        attachment: &crate::worker::WindowsUiaAttachment,\n        request: WindowsUiaVirtualizedItemRealizeRequest,\n    ) -> Result<WindowsUiaVirtualizedItemRealizeReceipt, crate::worker::WindowsUiaWorkerError> {\n        #[cfg(windows)]\n        {\n            self.realize_virtualized_item_on_mta(attachment, request)\n        }\n        #[cfg(not(windows))]\n        {\n            let _ = (attachment, request);\n            Err(crate::worker::WindowsUiaWorkerError::UnsupportedPlatform)\n        }\n    }\n'''
virtualized = replace_once(virtualized, old_realize, new_realize, "public realize delegate")
virtualized_path.write_text(virtualized, encoding="utf-8")

print("W03 worker patch applied deterministically")
