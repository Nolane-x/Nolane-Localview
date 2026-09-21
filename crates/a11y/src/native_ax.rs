use localview_native_provider::NativeSemanticNodeObservation;
use localview_protocol::{ElementRef, ProviderElementRealization, ProviderElementRef};

use crate::{MAX_NATIVE_AX_RECORDS, NativeAxEvidence, valid_stable_reference};

const MAX_NATIVE_TEXT_BYTES: usize = 120;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeAxStableRefBinding {
    pub provider_element: ProviderElementRef,
    pub stable_reference: ElementRef,
}

/// Adapt provider-normalized native semantic evidence into the narrow Wave 6 AX
/// enrichment packet.
///
/// The binding must match the full provider element identity, including
/// provider/target incarnation and acquisition cut. No semantic locator,
/// selector, role/name similarity, or opaque ID alone is allowed to mint a
/// LocalView stable reference.
pub fn adapt_native_ax(
    nodes: &[NativeSemanticNodeObservation],
    bindings: &[NativeAxStableRefBinding],
) -> Vec<NativeAxEvidence> {
    nodes
        .iter()
        .take(MAX_NATIVE_AX_RECORDS)
        .filter(|node| node.element_ref.realization == ProviderElementRealization::RealizedCurrent)
        .filter_map(|node| {
            let binding = bindings.iter().find(|binding| {
                binding.provider_element == node.element_ref
                    && valid_stable_reference(&binding.stable_reference)
            })?;
            let sensitive = parse_bool_attr(node, "windows_uia.is_password") == Some(true)
                || parse_bool_attr(node, "localview.sensitive") == Some(true);

            Some(NativeAxEvidence {
                stable_reference: Some(binding.stable_reference.clone()),
                role: node.role.as_deref().map(bounded_native_text),
                name: if sensitive {
                    None
                } else {
                    node.name.as_deref().map(bounded_native_text)
                },
                focusable: parse_first_bool(
                    node,
                    &[
                        "windows_uia.is_keyboard_focusable",
                        "macos_ax.focusable",
                        "atspi.focusable",
                    ],
                ),
                enabled: node.is_enabled,
                selected: parse_first_bool(
                    node,
                    &[
                        "windows_uia.selection_item.is_selected",
                        "macos_ax.selected",
                        "atspi.selected",
                    ],
                ),
                expanded: parse_expanded(node),
                offscreen: node.is_offscreen,
                // NativeSemanticNodeObservation does not currently expose one
                // cross-provider bounds authority. Do not synthesize it.
                bounds: None,
            })
        })
        .collect()
}

fn parse_expanded(node: &NativeSemanticNodeObservation) -> Option<bool> {
    if let Some(value) = node.attributes.get("windows_uia.expand_collapse.state") {
        return match value.as_str() {
            "0" | "collapsed" => Some(false),
            "1" | "expanded" => Some(true),
            _ => None,
        };
    }
    parse_first_bool(node, &["macos_ax.expanded", "atspi.expanded"])
}

fn parse_first_bool(node: &NativeSemanticNodeObservation, keys: &[&str]) -> Option<bool> {
    keys.iter().find_map(|key| parse_bool_attr(node, key))
}

fn parse_bool_attr(node: &NativeSemanticNodeObservation, key: &str) -> Option<bool> {
    match node.attributes.get(key)?.as_str() {
        "true" | "1" => Some(true),
        "false" | "0" => Some(false),
        _ => None,
    }
}

fn bounded_native_text(value: &str) -> String {
    if value.len() <= MAX_NATIVE_TEXT_BYTES {
        return value.to_owned();
    }
    let mut end = MAX_NATIVE_TEXT_BYTES;
    while !value.is_char_boundary(end) {
        end = end.saturating_sub(1);
    }
    value[..end].to_owned()
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use localview_protocol::{
        ProviderElementRealization, ProviderIncarnationRef, TargetIncarnationRef,
    };

    use super::*;

    fn provider_element(cut: &str) -> ProviderElementRef {
        ProviderElementRef {
            provider_family: "windows_uia".into(),
            provider_incarnation_ref: ProviderIncarnationRef::from("provider:wave6"),
            target_incarnation_ref: TargetIncarnationRef::from("target:wave6"),
            opaque_provider_element_id: "uia-runtime:[42]".into(),
            semantic_locator_hints: vec![],
            parent_surface_ref: Some("window:wave6".into()),
            acquisition_cut_ref: cut.into(),
            realization: ProviderElementRealization::RealizedCurrent,
            lifetime_profile_revision: "windows-uia-lifetime-v1".into(),
        }
    }

    fn node(cut: &str) -> NativeSemanticNodeObservation {
        NativeSemanticNodeObservation {
            element_ref: provider_element(cut),
            parent_index: None,
            depth: 0,
            role: Some("button".into()),
            name: Some("Save".into()),
            control_type: Some("Button".into()),
            automation_id: Some("save".into()),
            class_name: Some("Button".into()),
            is_enabled: Some(true),
            is_offscreen: Some(false),
            attributes: BTreeMap::from([
                (
                    "windows_uia.selection_item.is_selected".into(),
                    "true".into(),
                ),
                ("windows_uia.expand_collapse.state".into(), "1".into()),
            ]),
        }
    }

    #[test]
    fn exact_provider_identity_maps_to_stable_ref() {
        let evidence = adapt_native_ax(
            &[node("cut:7")],
            &[NativeAxStableRefBinding {
                provider_element: provider_element("cut:7"),
                stable_reference: "@eabc".into(),
            }],
        );
        assert_eq!(evidence.len(), 1);
        assert_eq!(evidence[0].stable_reference.as_deref(), Some("@eabc"));
        assert_eq!(evidence[0].role.as_deref(), Some("button"));
        assert_eq!(evidence[0].selected, Some(true));
        assert_eq!(evidence[0].expanded, Some(true));
    }

    #[test]
    fn stale_cut_never_maps_by_opaque_id_similarity() {
        let evidence = adapt_native_ax(
            &[node("cut:8")],
            &[NativeAxStableRefBinding {
                provider_element: provider_element("cut:7"),
                stable_reference: "@eabc".into(),
            }],
        );
        assert!(evidence.is_empty());
    }

    #[test]
    fn password_node_never_exports_native_name() {
        let mut password = node("cut:7");
        password.name = Some("private-account-name".into());
        password
            .attributes
            .insert("windows_uia.is_password".into(), "true".into());

        let evidence = adapt_native_ax(
            &[password],
            &[NativeAxStableRefBinding {
                provider_element: provider_element("cut:7"),
                stable_reference: "@eabc".into(),
            }],
        );
        assert_eq!(evidence.len(), 1);
        assert_eq!(evidence[0].name, None);
    }

    #[test]
    fn unrealized_provider_object_is_not_enrichment_authority() {
        let mut virtual_node = node("cut:7");
        virtual_node.element_ref.realization = ProviderElementRealization::RealizationRequired;
        let evidence = adapt_native_ax(
            &[virtual_node],
            &[NativeAxStableRefBinding {
                provider_element: provider_element("cut:7"),
                stable_reference: "@eabc".into(),
            }],
        );
        assert!(evidence.is_empty());
    }
}
