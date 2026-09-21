#![forbid(unsafe_code)]

pub mod native_ax;

use std::collections::{BTreeMap, BTreeSet};

use localview_protocol::{ElementRef, Rect, SemanticNode};
use serde::{Deserialize, Serialize};

pub const MAX_AXE_FINDINGS: usize = 128;
pub const MAX_NATIVE_AX_RECORDS: usize = 256;
pub const MAX_FINDING_MESSAGE_BYTES: usize = 240;
pub const MIN_NOMINAL_TARGET_PX: f64 = 24.0;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum A11yEvidenceKind {
    #[default]
    LocalDeterministic,
    AxeRule,
    NativeAx,
    Heuristic,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum TargetResolution {
    StableRef(ElementRef),
    #[default]
    Unresolved,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct A11yDiscrepancy {
    pub field: String,
    pub dom_value: Option<String>,
    pub native_value: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct A11yFinding {
    pub code: String,
    pub reference: String,
    pub message: String,
    pub deterministic: bool,
    pub confidence: u8,
    #[serde(default)]
    pub evidence_kind: A11yEvidenceKind,
    #[serde(default)]
    pub target_resolution: TargetResolution,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rule_id: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub discrepancies: Vec<A11yDiscrepancy>,
}

impl A11yFinding {
    fn local(
        code: &str,
        reference: &str,
        message: String,
        deterministic: bool,
        confidence: u8,
    ) -> Self {
        Self {
            code: code.into(),
            reference: reference.into(),
            message: bounded_text(&message, MAX_FINDING_MESSAGE_BYTES),
            deterministic,
            confidence,
            evidence_kind: if deterministic {
                A11yEvidenceKind::LocalDeterministic
            } else {
                A11yEvidenceKind::Heuristic
            },
            target_resolution: TargetResolution::StableRef(reference.into()),
            rule_id: None,
            discrepancies: Vec::new(),
        }
    }
}

pub fn audit(root: &SemanticNode) -> Vec<A11yFinding> {
    let mut out = Vec::new();
    walk(root, &mut out);
    out
}

fn walk(node: &SemanticNode, out: &mut Vec<A11yFinding>) {
    let role = node.role.as_deref().unwrap_or("");
    if node.interactive && node.name.as_deref().unwrap_or("").trim().is_empty() {
        out.push(A11yFinding::local(
            "interactive_name_missing",
            &node.reference,
            "Interactive control has no accessible name".into(),
            true,
            100,
        ));
    }
    if role == "img" && node.name.as_deref().unwrap_or("").trim().is_empty() {
        out.push(A11yFinding::local(
            "image_name_missing",
            &node.reference,
            "Image lacks an accessible alternative".into(),
            true,
            100,
        ));
    }
    if let Some(rect) = &node.rect {
        if node.interactive
            && (rect.width < MIN_NOMINAL_TARGET_PX || rect.height < MIN_NOMINAL_TARGET_PX)
        {
            out.push(A11yFinding::local(
                "small_nominal_hit_target",
                &node.reference,
                format!(
                    "Nominal hit target is {:.0}×{:.0}px",
                    rect.width, rect.height
                ),
                false,
                84,
            ));
        }
    }
    for child in &node.children {
        walk(child, out);
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AxeRuleNode {
    pub rule_id: String,
    pub impact: Option<String>,
    pub help: String,
    pub stable_reference: Option<ElementRef>,
    pub exact_reference_mapping: bool,
}

pub fn normalize_axe_findings(nodes: &[AxeRuleNode], max_findings: usize) -> Vec<A11yFinding> {
    let cap = max_findings.min(MAX_AXE_FINDINGS);
    nodes
        .iter()
        .take(cap)
        .map(|node| {
            let resolved = node.exact_reference_mapping
                && node
                    .stable_reference
                    .as_deref()
                    .is_some_and(valid_stable_reference);
            let reference = if resolved {
                node.stable_reference.clone().unwrap_or_default()
            } else {
                String::new()
            };
            A11yFinding {
                code: bounded_identifier(&node.rule_id, 96),
                reference: reference.clone(),
                message: bounded_text(&node.help, MAX_FINDING_MESSAGE_BYTES),
                deterministic: false,
                confidence: if resolved { 96 } else { 88 },
                evidence_kind: A11yEvidenceKind::AxeRule,
                target_resolution: if resolved {
                    TargetResolution::StableRef(reference)
                } else {
                    TargetResolution::Unresolved
                },
                rule_id: Some(bounded_identifier(&node.rule_id, 96)),
                discrepancies: Vec::new(),
            }
        })
        .collect()
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct NativeAxEvidence {
    pub stable_reference: Option<ElementRef>,
    pub role: Option<String>,
    pub name: Option<String>,
    pub focusable: Option<bool>,
    pub enabled: Option<bool>,
    pub selected: Option<bool>,
    pub expanded: Option<bool>,
    pub offscreen: Option<bool>,
    pub bounds: Option<Rect>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DomA11yEvidence {
    pub reference: ElementRef,
    pub role: Option<String>,
    pub name: Option<String>,
    pub focusable: Option<bool>,
    pub enabled: Option<bool>,
    pub selected: Option<bool>,
    pub expanded: Option<bool>,
    pub offscreen: Option<bool>,
    pub bounds: Option<Rect>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct NativeAxEnrichment {
    pub reference: ElementRef,
    pub native: NativeAxEvidence,
    pub discrepancies: Vec<A11yDiscrepancy>,
}

pub fn native_ax_discrepancy_findings(enrichments: &[NativeAxEnrichment]) -> Vec<A11yFinding> {
    enrichments
        .iter()
        .filter(|entry| !entry.discrepancies.is_empty())
        .take(MAX_NATIVE_AX_RECORDS)
        .map(|entry| {
            let fields = entry
                .discrepancies
                .iter()
                .map(|item| item.field.as_str())
                .take(8)
                .collect::<Vec<_>>()
                .join(",");
            A11yFinding {
                code: "dom_native_ax_discrepancy".into(),
                reference: entry.reference.clone(),
                message: bounded_text(
                    &format!("DOM/native AX evidence differs for: {fields}"),
                    MAX_FINDING_MESSAGE_BYTES,
                ),
                deterministic: false,
                confidence: 92,
                evidence_kind: A11yEvidenceKind::NativeAx,
                target_resolution: TargetResolution::StableRef(entry.reference.clone()),
                rule_id: None,
                discrepancies: entry.discrepancies.clone(),
            }
        })
        .collect()
}

pub fn enrich_native_ax(
    dom: &[DomA11yEvidence],
    native: &[NativeAxEvidence],
) -> Vec<NativeAxEnrichment> {
    let dom_by_ref: BTreeMap<&str, &DomA11yEvidence> = dom
        .iter()
        .map(|entry| (entry.reference.as_str(), entry))
        .collect();

    native
        .iter()
        .take(MAX_NATIVE_AX_RECORDS)
        .filter_map(|native_entry| {
            let reference = native_entry.stable_reference.as_deref()?;
            if !valid_stable_reference(reference) {
                return None;
            }
            let dom_entry = dom_by_ref.get(reference)?;
            let mut discrepancies = Vec::new();
            discrepancy(
                &mut discrepancies,
                "role",
                dom_entry.role.as_deref(),
                native_entry.role.as_deref(),
            );
            discrepancy(
                &mut discrepancies,
                "name",
                dom_entry.name.as_deref(),
                native_entry.name.as_deref(),
            );
            discrepancy_bool(
                &mut discrepancies,
                "focusable",
                dom_entry.focusable,
                native_entry.focusable,
            );
            discrepancy_bool(
                &mut discrepancies,
                "enabled",
                dom_entry.enabled,
                native_entry.enabled,
            );
            discrepancy_bool(
                &mut discrepancies,
                "selected",
                dom_entry.selected,
                native_entry.selected,
            );
            discrepancy_bool(
                &mut discrepancies,
                "expanded",
                dom_entry.expanded,
                native_entry.expanded,
            );
            discrepancy_bool(
                &mut discrepancies,
                "offscreen",
                dom_entry.offscreen,
                native_entry.offscreen,
            );
            if let (Some(dom_bounds), Some(native_bounds)) =
                (&dom_entry.bounds, &native_entry.bounds)
            {
                let delta = (dom_bounds.x - native_bounds.x).abs()
                    + (dom_bounds.y - native_bounds.y).abs()
                    + (dom_bounds.width - native_bounds.width).abs()
                    + (dom_bounds.height - native_bounds.height).abs();
                if delta > 2.0 {
                    discrepancies.push(A11yDiscrepancy {
                        field: "bounds".into(),
                        dom_value: Some(rect_summary(dom_bounds)),
                        native_value: Some(rect_summary(native_bounds)),
                    });
                }
            }
            Some(NativeAxEnrichment {
                reference: reference.into(),
                native: native_entry.clone(),
                discrepancies,
            })
        })
        .collect()
}

fn discrepancy(
    out: &mut Vec<A11yDiscrepancy>,
    field: &str,
    dom: Option<&str>,
    native: Option<&str>,
) {
    let dom = normalize_nonempty(dom);
    let native = normalize_nonempty(native);
    if dom.is_some() && native.is_some() && dom != native {
        out.push(A11yDiscrepancy {
            field: field.into(),
            dom_value: dom.map(|value| bounded_text(value, 120)),
            native_value: native.map(|value| bounded_text(value, 120)),
        });
    }
}

fn normalize_nonempty(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|value| !value.is_empty())
}

fn discrepancy_bool(
    out: &mut Vec<A11yDiscrepancy>,
    field: &str,
    dom: Option<bool>,
    native: Option<bool>,
) {
    if let (Some(dom), Some(native)) = (dom, native) {
        if dom != native {
            out.push(A11yDiscrepancy {
                field: field.into(),
                dom_value: Some(dom.to_string()),
                native_value: Some(native.to_string()),
            });
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum HitTestAuthority {
    GeometryOnly,
    BrowserHitTest,
    NativePointerHitTest,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct HitboxEvidence {
    pub reference: ElementRef,
    pub nominal: Rect,
    pub visible_clip: Option<Rect>,
    pub pointer_events: Option<bool>,
    pub occluders: Vec<ElementRef>,
    pub delivery_observed: Option<bool>,
    pub authority: HitTestAuthority,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EffectiveHitbox {
    pub reference: ElementRef,
    pub nominal_area: f64,
    pub effective_area: f64,
    pub effective_rect: Option<Rect>,
    pub suspected_blocked: bool,
    pub deterministically_blocked: bool,
    pub authority: HitTestAuthority,
    pub evidence_refs: Vec<ElementRef>,
}

pub fn effective_hitbox(input: &HitboxEvidence) -> EffectiveHitbox {
    let nominal_area = area(&input.nominal);
    let clipped = input
        .visible_clip
        .as_ref()
        .and_then(|clip| intersect(&input.nominal, clip))
        .or_else(|| input.visible_clip.is_none().then(|| input.nominal.clone()));
    let effective_area = clipped.as_ref().map_or(0.0, area);
    let pointer_blocked = input.pointer_events == Some(false);
    let occluded = !input.occluders.is_empty();
    let delivery_blocked = input.delivery_observed == Some(false);
    let strong_authority = matches!(
        input.authority,
        HitTestAuthority::BrowserHitTest | HitTestAuthority::NativePointerHitTest
    );
    let deterministically_blocked =
        strong_authority && (delivery_blocked || pointer_blocked || effective_area <= 0.0);
    let suspected_blocked =
        deterministically_blocked || pointer_blocked || occluded || effective_area < nominal_area;

    let evidence_refs = input
        .occluders
        .iter()
        .filter(|reference| valid_stable_reference(reference))
        .take(16)
        .cloned()
        .collect();

    EffectiveHitbox {
        reference: input.reference.clone(),
        nominal_area,
        effective_area,
        effective_rect: clipped,
        suspected_blocked,
        deterministically_blocked,
        authority: input.authority,
        evidence_refs,
    }
}

fn intersect(a: &Rect, b: &Rect) -> Option<Rect> {
    let x1 = a.x.max(b.x);
    let y1 = a.y.max(b.y);
    let x2 = (a.x + a.width).min(b.x + b.width);
    let y2 = (a.y + a.height).min(b.y + b.height);
    if x2 <= x1 || y2 <= y1 {
        return None;
    }
    Some(Rect {
        x: x1,
        y: y1,
        width: x2 - x1,
        height: y2 - y1,
    })
}

fn area(rect: &Rect) -> f64 {
    if !rect.width.is_finite()
        || !rect.height.is_finite()
        || rect.width <= 0.0
        || rect.height <= 0.0
    {
        0.0
    } else {
        rect.width * rect.height
    }
}

fn rect_summary(rect: &Rect) -> String {
    format!(
        "{:.1},{:.1} {:.1}×{:.1}",
        rect.x, rect.y, rect.width, rect.height
    )
}

pub fn valid_stable_reference(reference: &str) -> bool {
    let Some(rest) = reference.strip_prefix("@e") else {
        return false;
    };
    !rest.is_empty() && rest.len() <= 64 && rest.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn bounded_identifier(value: &str, max_bytes: usize) -> String {
    let filtered: String = value
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.'))
        .collect();
    bounded_text(&filtered, max_bytes)
}

fn bounded_text(value: &str, max_bytes: usize) -> String {
    if value.len() <= max_bytes {
        return value.to_owned();
    }
    let mut end = max_bytes;
    while !value.is_char_boundary(end) {
        end = end.saturating_sub(1);
    }
    value[..end].to_owned()
}

pub fn unique_finding_codes(findings: &[A11yFinding]) -> BTreeSet<String> {
    findings
        .iter()
        .map(|finding| finding.code.clone())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn node(reference: &str, role: &str, name: Option<&str>, interactive: bool) -> SemanticNode {
        SemanticNode {
            reference: reference.into(),
            role: Some(role.into()),
            name: name.map(str::to_owned),
            tag: role.into(),
            rect: None,
            interactive,
            attributes: BTreeMap::new(),
            source: None,
            ownership: None,
            children: vec![],
        }
    }

    #[test]
    fn nameless_button_is_flagged_as_local_deterministic() {
        let finding = audit(&node("@e1", "button", None, true)).remove(0);
        assert_eq!(finding.code, "interactive_name_missing");
        assert!(finding.deterministic);
        assert_eq!(finding.evidence_kind, A11yEvidenceKind::LocalDeterministic);
    }

    #[test]
    fn image_alt_is_checked_without_reusing_page_text() {
        let finding = audit(&node("@e2", "img", None, false)).remove(0);
        assert_eq!(finding.code, "image_name_missing");
        assert!(!finding.message.contains("<"));
    }

    #[test]
    fn small_target_is_nominal_geometry_suspicion_not_delivery_proof() {
        let mut target = node("@e3", "button", Some("Tiny"), true);
        target.rect = Some(Rect {
            x: 0.0,
            y: 0.0,
            width: 12.0,
            height: 18.0,
        });
        let finding = audit(&target)
            .into_iter()
            .find(|finding| finding.code == "small_nominal_hit_target")
            .expect("small nominal target finding");
        assert!(!finding.deterministic);
        assert_eq!(finding.evidence_kind, A11yEvidenceKind::Heuristic);
    }

    #[test]
    fn axe_unresolved_target_never_fabricates_reference() {
        let findings = normalize_axe_findings(
            &[AxeRuleNode {
                rule_id: "button-name".into(),
                impact: Some("critical".into()),
                help: "Buttons must have discernible text".into(),
                stable_reference: Some("#save > span".into()),
                exact_reference_mapping: false,
            }],
            8,
        );
        assert_eq!(findings[0].reference, "");
        assert_eq!(findings[0].target_resolution, TargetResolution::Unresolved);
        assert_eq!(findings[0].evidence_kind, A11yEvidenceKind::AxeRule);
    }

    #[test]
    fn axe_output_is_hard_bounded() {
        let input = (0..256)
            .map(|idx| AxeRuleNode {
                rule_id: format!("rule-{idx}"),
                impact: None,
                help: "x".repeat(512),
                stable_reference: Some("@eabc".into()),
                exact_reference_mapping: true,
            })
            .collect::<Vec<_>>();
        let findings = normalize_axe_findings(&input, usize::MAX);
        assert_eq!(findings.len(), MAX_AXE_FINDINGS);
        assert!(
            findings
                .iter()
                .all(|finding| finding.message.len() <= MAX_FINDING_MESSAGE_BYTES)
        );
    }

    #[test]
    fn native_ax_conflict_is_retained_as_discrepancy() {
        let dom = vec![DomA11yEvidence {
            reference: "@eabc".into(),
            role: Some("button".into()),
            name: Some("Save".into()),
            focusable: Some(true),
            enabled: Some(true),
            selected: None,
            expanded: Some(false),
            offscreen: Some(false),
            bounds: None,
        }];
        let native = vec![NativeAxEvidence {
            stable_reference: Some("@eabc".into()),
            role: Some("checkbox".into()),
            name: Some("Save".into()),
            focusable: Some(true),
            enabled: Some(true),
            selected: None,
            expanded: Some(true),
            offscreen: Some(false),
            bounds: None,
        }];
        let enriched = enrich_native_ax(&dom, &native);
        assert_eq!(enriched.len(), 1);
        assert!(
            enriched[0]
                .discrepancies
                .iter()
                .any(|item| item.field == "role")
        );
        assert!(
            enriched[0]
                .discrepancies
                .iter()
                .any(|item| item.field == "expanded")
        );
    }

    #[test]
    fn native_ax_discrepancy_is_classified_as_native_evidence() {
        let findings = native_ax_discrepancy_findings(&[NativeAxEnrichment {
            reference: "@eabc".into(),
            native: NativeAxEvidence {
                stable_reference: Some("@eabc".into()),
                role: Some("checkbox".into()),
                name: Some("Save".into()),
                focusable: Some(true),
                enabled: Some(true),
                selected: None,
                expanded: None,
                offscreen: Some(false),
                bounds: None,
            },
            discrepancies: vec![A11yDiscrepancy {
                field: "role".into(),
                dom_value: Some("button".into()),
                native_value: Some("checkbox".into()),
            }],
        }]);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].evidence_kind, A11yEvidenceKind::NativeAx);
        assert!(!findings[0].deterministic);
        assert_eq!(findings[0].reference, "@eabc");
    }

    #[test]
    fn geometry_only_occlusion_stays_suspected_not_deterministic() {
        let result = effective_hitbox(&HitboxEvidence {
            reference: "@eabc".into(),
            nominal: Rect {
                x: 0.0,
                y: 0.0,
                width: 20.0,
                height: 20.0,
            },
            visible_clip: Some(Rect {
                x: 0.0,
                y: 0.0,
                width: 10.0,
                height: 10.0,
            }),
            pointer_events: Some(true),
            occluders: vec!["@edef".into()],
            delivery_observed: None,
            authority: HitTestAuthority::GeometryOnly,
        });
        assert!(result.suspected_blocked);
        assert!(!result.deterministically_blocked);
        assert_eq!(result.effective_area, 100.0);
    }

    #[test]
    fn browser_hit_test_can_prove_blocked_delivery() {
        let result = effective_hitbox(&HitboxEvidence {
            reference: "@eabc".into(),
            nominal: Rect {
                x: 0.0,
                y: 0.0,
                width: 40.0,
                height: 40.0,
            },
            visible_clip: None,
            pointer_events: Some(true),
            occluders: vec![],
            delivery_observed: Some(false),
            authority: HitTestAuthority::BrowserHitTest,
        });
        assert!(result.deterministically_blocked);
    }
}
