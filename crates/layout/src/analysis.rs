use std::collections::{BTreeMap, BTreeSet};

use localview_protocol::{ElementRef, Rect};

use crate::model::{
    DisplayMode, LayoutAnalysis, LayoutElement, LayoutFact, LayoutIssue, LayoutIssueClass,
    OverflowMode, PositionMode, Severity, SpacingFamily, SpacingSource,
};

pub const MAX_LAYOUT_ELEMENTS: usize = 512;
const MAX_LAYOUT_ISSUES: usize = 256;
const MAX_LAYOUT_FACTS: usize = 256;
const MAX_SPACING_SAMPLES: usize = 2_048;
const GEOMETRY_EPSILON: f64 = 0.5;
const SPACING_CLUSTER_EPSILON: f64 = 0.75;
const ALIGNMENT_CLUSTER_EPSILON: f64 = 1.0;
const ALIGNMENT_OUTLIER_THRESHOLD: f64 = 2.5;
const SIBLING_OVERLAP_THRESHOLD: f64 = 0.35;
const SUBSTANTIAL_OVERLAP_THRESHOLD: f64 = 0.60;

#[derive(Debug, Clone)]
struct SpacingSample {
    value: f64,
    source: SpacingSource,
    parent: Option<ElementRef>,
    refs: Vec<ElementRef>,
}

#[derive(Debug, Clone)]
struct AlignmentCandidate {
    reference: ElementRef,
    family: &'static str,
    expected: f64,
    measured: f64,
    deviation: f64,
    support: usize,
}

pub fn analyze(elements: &[LayoutElement], viewport: (f64, f64)) -> LayoutAnalysis {
    let input_truncated = elements.len() > MAX_LAYOUT_ELEMENTS;
    let elements = &elements[..elements.len().min(MAX_LAYOUT_ELEMENTS)];
    let mut result = LayoutAnalysis {
        analyzed_nodes: elements.len(),
        input_truncated,
        ..Default::default()
    };

    if !finite_positive(viewport.0) || !finite_positive(viewport.1) {
        push_issue(
            &mut result.issues,
            LayoutIssue {
                code: "invalid_viewport_geometry".into(),
                severity: Severity::Error,
                confidence: 1.0,
                class: LayoutIssueClass::Deterministic,
                refs: Vec::new(),
                message: "Viewport geometry is invalid; layout audit failed closed".into(),
                evidence: format!("viewport={}x{}", viewport.0, viewport.1),
            },
        );
        return result;
    }

    let index = elements
        .iter()
        .enumerate()
        .map(|(position, element)| (element.reference.clone(), position))
        .collect::<BTreeMap<_, _>>();

    let valid = elements
        .iter()
        .map(|element| classify_geometry(element, &mut result.issues))
        .collect::<Vec<_>>();

    record_container_facts(elements, &valid, &mut result.facts);
    analyze_overflow(elements, &valid, &index, viewport, &mut result);
    analyze_occlusion(elements, &valid, &index, &mut result.issues);
    analyze_collisions(elements, &valid, &index, &mut result.issues);

    let samples = spacing_samples(elements, &valid, &index);
    result.spacing_families = infer_spacing_families(&samples);
    analyze_spacing_outliers(&samples, &mut result.issues);
    analyze_alignment(elements, &valid, &mut result.issues);

    result.issues.sort_by(|left, right| {
        severity_rank(right.severity)
            .cmp(&severity_rank(left.severity))
            .then_with(|| right.confidence.total_cmp(&left.confidence))
            .then_with(|| left.code.cmp(&right.code))
            .then_with(|| left.refs.cmp(&right.refs))
    });
    result
}

pub fn audit(elements: &[LayoutElement], viewport: (f64, f64)) -> Vec<LayoutIssue> {
    analyze(elements, viewport).issues
}

pub fn infer_spacing_scale(values: &[f64]) -> Vec<f64> {
    let mut values = values
        .iter()
        .copied()
        .filter(|value| finite_positive(*value))
        .take(MAX_SPACING_SAMPLES)
        .collect::<Vec<_>>();
    values.sort_by(f64::total_cmp);
    cluster_values(&values, SPACING_CLUSTER_EPSILON)
        .into_iter()
        .map(|cluster| mean(&cluster))
        .collect()
}

fn classify_geometry(element: &LayoutElement, issues: &mut Vec<LayoutIssue>) -> bool {
    let rect = &element.rect;
    if !rect.x.is_finite()
        || !rect.y.is_finite()
        || !rect.width.is_finite()
        || !rect.height.is_finite()
        || !rect_right(rect).is_finite()
        || !rect_bottom(rect).is_finite()
        || rect.width < 0.0
        || rect.height < 0.0
    {
        push_issue(
            issues,
            LayoutIssue {
                code: "invalid_geometry".into(),
                severity: Severity::Error,
                confidence: 1.0,
                class: LayoutIssueClass::Deterministic,
                refs: vec![element.reference.clone()],
                message: "Element geometry is invalid and was excluded from relational analysis".into(),
                evidence: format!("rect={rect:?}"),
            },
        );
        return false;
    }
    if rect.width <= 0.0 || rect.height <= 0.0 {
        push_issue(
            issues,
            LayoutIssue {
                code: "zero_area".into(),
                severity: Severity::Warning,
                confidence: 1.0,
                class: LayoutIssueClass::Deterministic,
                refs: vec![element.reference.clone()],
                message: "Element has zero visual area".into(),
                evidence: format!("width={:.3} height={:.3}", rect.width, rect.height),
            },
        );
        return false;
    }
    true
}

fn record_container_facts(elements: &[LayoutElement], valid: &[bool], facts: &mut Vec<LayoutFact>) {
    for (element, is_valid) in elements.iter().zip(valid.iter().copied()) {
        if !is_valid {
            continue;
        }
        match element.style.display {
            DisplayMode::Flex | DisplayMode::InlineFlex => push_fact(
                facts,
                LayoutFact {
                    code: "flex_container".into(),
                    refs: vec![element.reference.clone()],
                    evidence: format!(
                        "display={:?} direction={:?} wrap={:?} justify={:?} align={:?} row_gap={:?} column_gap={:?}",
                        element.style.display,
                        element.style.flex_direction,
                        element.style.flex_wrap,
                        element.style.justify_content,
                        element.style.align_items,
                        element.style.row_gap,
                        element.style.column_gap
                    ),
                },
            ),
            DisplayMode::Grid | DisplayMode::InlineGrid => push_fact(
                facts,
                LayoutFact {
                    code: "grid_container".into(),
                    refs: vec![element.reference.clone()],
                    evidence: format!(
                        "display={:?} columns={:?} rows={:?} row_gap={:?} column_gap={:?}",
                        element.style.display,
                        element.style.grid_template_columns,
                        element.style.grid_template_rows,
                        element.style.row_gap,
                        element.style.column_gap
                    ),
                },
            ),
            _ => {}
        }
    }
}

fn analyze_overflow(
    elements: &[LayoutElement],
    valid: &[bool],
    index: &BTreeMap<ElementRef, usize>,
    viewport: (f64, f64),
    result: &mut LayoutAnalysis,
) {
    for (position, element) in elements.iter().enumerate() {
        if !valid[position] {
            continue;
        }

        if let Some(parent_ref) = &element.parent {
            if let Some(&parent_position) = index.get(parent_ref) {
                if valid.get(parent_position).copied().unwrap_or(false) {
                    let parent = &elements[parent_position];
                    let (overflow_x, overflow_y) = outside_rect(&element.rect, &parent.rect);
                    if overflow_x || overflow_y {
                        let constrained_x = overflow_x
                            && parent.style.overflow_x.is_some_and(OverflowMode::constrains);
                        let constrained_y = overflow_y
                            && parent.style.overflow_y.is_some_and(OverflowMode::constrains);
                        let clipped = (overflow_x
                            && parent.style.overflow_x.is_some_and(OverflowMode::clips))
                            || (overflow_y
                                && parent.style.overflow_y.is_some_and(OverflowMode::clips));
                        let scrolled = (overflow_x
                            && parent.style.overflow_x.is_some_and(OverflowMode::scrolls))
                            || (overflow_y
                                && parent.style.overflow_y.is_some_and(OverflowMode::scrolls));

                        if clipped || element.visibility.clipped == Some(true) {
                            push_fact(
                                &mut result.facts,
                                LayoutFact {
                                    code: "intentional_clip".into(),
                                    refs: vec![parent.reference.clone(), element.reference.clone()],
                                    evidence: format!(
                                        "overflow_x={overflow_x} overflow_y={overflow_y} parent_overflow_x={:?} parent_overflow_y={:?}",
                                        parent.style.overflow_x, parent.style.overflow_y
                                    ),
                                },
                            );
                        }
                        if scrolled {
                            push_fact(
                                &mut result.facts,
                                LayoutFact {
                                    code: "scroll_container_overflow".into(),
                                    refs: vec![parent.reference.clone(), element.reference.clone()],
                                    evidence: format!(
                                        "overflow_x={overflow_x} overflow_y={overflow_y} parent_overflow_x={:?} parent_overflow_y={:?}",
                                        parent.style.overflow_x, parent.style.overflow_y
                                    ),
                                },
                            );
                        }

                        let accidental_x = overflow_x
                            && parent.style.overflow_x == Some(OverflowMode::Visible)
                            && !constrained_x;
                        let accidental_y = overflow_y
                            && parent.style.overflow_y == Some(OverflowMode::Visible)
                            && !constrained_y;
                        if (accidental_x || accidental_y)
                            && element.visibility.clipped != Some(true)
                        {
                            push_issue(
                                &mut result.issues,
                                LayoutIssue {
                                    code: "container_overflow".into(),
                                    severity: Severity::Warning,
                                    confidence: 1.0,
                                    class: LayoutIssueClass::Deterministic,
                                    refs: vec![parent.reference.clone(), element.reference.clone()],
                                    message: "Element extends beyond a parent whose observed overflow axis is visible".into(),
                                    evidence: format!(
                                        "child={:?} parent={:?} overflow_x={overflow_x} overflow_y={overflow_y} accidental_x={accidental_x} accidental_y={accidental_y}",
                                        element.rect, parent.rect
                                    ),
                                },
                            );
                        }
                    }
                }
            }
        }

        let (viewport_x, viewport_y) = outside_viewport(&element.rect, viewport);
        if !(viewport_x || viewport_y) || element.visibility.in_viewport == Some(false) {
            continue;
        }
        let constrained_x = viewport_x && has_constraining_ancestor(elements, index, element, true);
        let constrained_y = viewport_y && has_constraining_ancestor(elements, index, element, false);
        let uncontained_x = viewport_x && !constrained_x;
        let uncontained_y = viewport_y && !constrained_y;
        if !(uncontained_x || uncontained_y) || element.visibility.clipped == Some(true) {
            continue;
        }
        push_issue(
            &mut result.issues,
            LayoutIssue {
                code: "viewport_overflow".into(),
                severity: Severity::Warning,
                confidence: 1.0,
                class: LayoutIssueClass::Deterministic,
                refs: vec![element.reference.clone()],
                message: "Element extends beyond the observed viewport without a constraining ancestor on that axis".into(),
                evidence: format!(
                    "rect={:?} viewport={}x{} overflow_x={viewport_x} overflow_y={viewport_y}",
                    element.rect, viewport.0, viewport.1
                ),
            },
        );
    }
}

fn analyze_occlusion(
    elements: &[LayoutElement],
    valid: &[bool],
    index: &BTreeMap<ElementRef, usize>,
    issues: &mut Vec<LayoutIssue>,
) {
    for (position, element) in elements.iter().enumerate() {
        if !valid[position]
            || !element.interactive
            || !element.visibility.sampled
            || element.visibility.occluded != Some(true)
        {
            continue;
        }
        let Some(blocker_ref) = element.visibility.occluded_by.as_ref() else {
            continue;
        };
        let Some(&blocker_position) = index.get(blocker_ref) else {
            continue;
        };
        if !valid.get(blocker_position).copied().unwrap_or(false) {
            continue;
        }
        let blocker = &elements[blocker_position];
        push_issue(
            issues,
            LayoutIssue {
                code: "control_occluded".into(),
                severity: Severity::Error,
                confidence: 1.0,
                class: LayoutIssueClass::Deterministic,
                refs: vec![element.reference.clone(), blocker.reference.clone()],
                message: "Interactive control center-point was observed behind another region".into(),
                evidence: format!(
                    "sampled=true occluded_by={} blocker_position={:?} blocker_z_index={:?}",
                    blocker.reference, blocker.style.position, blocker.style.z_index
                ),
            },
        );
    }
}

fn analyze_collisions(
    elements: &[LayoutElement],
    valid: &[bool],
    index: &BTreeMap<ElementRef, usize>,
    issues: &mut Vec<LayoutIssue>,
) {
    for left_position in 0..elements.len() {
        if !valid[left_position] {
            continue;
        }
        for right_position in (left_position + 1)..elements.len() {
            if !valid[right_position] {
                continue;
            }
            let left = &elements[left_position];
            let right = &elements[right_position];
            if is_ancestor(index, elements, &left.reference, &right.reference)
                || is_ancestor(index, elements, &right.reference, &left.reference)
            {
                continue;
            }
            let overlap = meaningful_overlap(&left.rect, &right.rect);
            if overlap <= 0.0 {
                continue;
            }

            let fixed_or_sticky = is_fixed_or_sticky(left) || is_fixed_or_sticky(right);
            let authority = occlusion_pair_authority(left, right);
            if fixed_or_sticky && overlap >= 0.10 && authority {
                push_issue(
                    issues,
                    LayoutIssue {
                        code: "fixed_sticky_collision".into(),
                        severity: Severity::Error,
                        confidence: 1.0,
                        class: LayoutIssueClass::Deterministic,
                        refs: vec![left.reference.clone(), right.reference.clone()],
                        message: "Fixed/sticky overlap is corroborated by sampled occlusion evidence".into(),
                        evidence: format!(
                            "overlap_ratio={overlap:.3} left_position={:?} right_position={:?} left_z_index={:?} right_z_index={:?}",
                            left.style.position,
                            right.style.position,
                            left.style.z_index,
                            right.style.z_index
                        ),
                    },
                );
                continue;
            }

            if left.parent.is_some() && left.parent == right.parent && overlap >= SIBLING_OVERLAP_THRESHOLD {
                push_issue(
                    issues,
                    LayoutIssue {
                        code: "sibling_collision".into(),
                        severity: Severity::Warning,
                        confidence: 0.90,
                        class: LayoutIssueClass::Heuristic,
                        refs: vec![left.reference.clone(), right.reference.clone()],
                        message: "Sibling geometry overlaps substantially".into(),
                        evidence: format!("overlap_ratio={overlap:.3}"),
                    },
                );
            } else if overlap >= SUBSTANTIAL_OVERLAP_THRESHOLD
                && (left.interactive || right.interactive)
            {
                push_issue(
                    issues,
                    LayoutIssue {
                        code: "substantial_overlap".into(),
                        severity: Severity::Warning,
                        confidence: 0.78,
                        class: LayoutIssueClass::Heuristic,
                        refs: vec![left.reference.clone(), right.reference.clone()],
                        message: "Unrelated visible regions overlap substantially; intent is not proven".into(),
                        evidence: format!("overlap_ratio={overlap:.3}"),
                    },
                );
            }
        }
    }
}

fn spacing_samples(
    elements: &[LayoutElement],
    valid: &[bool],
    index: &BTreeMap<ElementRef, usize>,
) -> Vec<SpacingSample> {
    let mut samples = Vec::new();

    for (position, element) in elements.iter().enumerate() {
        if !valid[position] {
            continue;
        }
        if let Some(padding) = element.padding {
            for value in padding {
                push_spacing_sample(
                    &mut samples,
                    SpacingSample {
                        value,
                        source: SpacingSource::Padding,
                        parent: Some(element.reference.clone()),
                        refs: vec![element.reference.clone()],
                    },
                );
            }
        }
    }

    let groups = children_by_parent(elements, valid);
    for (parent_ref, children) in groups {
        let parent = index.get(&parent_ref).and_then(|position| elements.get(*position));
        if let Some(parent) = parent {
            for &child_position in &children {
                let child = &elements[child_position];
                for value in [
                    child.rect.x - parent.rect.x,
                    rect_right(&parent.rect) - rect_right(&child.rect),
                    child.rect.y - parent.rect.y,
                    rect_bottom(&parent.rect) - rect_bottom(&child.rect),
                ] {
                    if value >= -GEOMETRY_EPSILON {
                        push_spacing_sample(
                            &mut samples,
                            SpacingSample {
                                value: value.max(0.0),
                                source: SpacingSource::EdgeDistance,
                                parent: Some(parent_ref.clone()),
                                refs: vec![parent_ref.clone(), child.reference.clone()],
                            },
                        );
                    }
                }
            }
        }

        let mut by_x = children.clone();
        by_x.sort_by(|left, right| elements[*left].rect.x.total_cmp(&elements[*right].rect.x));
        for pair in by_x.windows(2) {
            let left = &elements[pair[0]];
            let right = &elements[pair[1]];
            if ranges_overlap(
                left.rect.y,
                rect_bottom(&left.rect),
                right.rect.y,
                rect_bottom(&right.rect),
            ) {
                let gap = right.rect.x - rect_right(&left.rect);
                if gap >= -GEOMETRY_EPSILON {
                    push_spacing_sample(
                        &mut samples,
                        SpacingSample {
                            value: gap.max(0.0),
                            source: SpacingSource::SiblingGap,
                            parent: Some(parent_ref.clone()),
                            refs: vec![left.reference.clone(), right.reference.clone()],
                        },
                    );
                }
            }
        }

        let mut by_y = children;
        by_y.sort_by(|top, bottom| elements[*top].rect.y.total_cmp(&elements[*bottom].rect.y));
        for pair in by_y.windows(2) {
            let top = &elements[pair[0]];
            let bottom = &elements[pair[1]];
            if ranges_overlap(
                top.rect.x,
                rect_right(&top.rect),
                bottom.rect.x,
                rect_right(&bottom.rect),
            ) {
                let gap = bottom.rect.y - rect_bottom(&top.rect);
                if gap >= -GEOMETRY_EPSILON {
                    push_spacing_sample(
                        &mut samples,
                        SpacingSample {
                            value: gap.max(0.0),
                            source: SpacingSource::SiblingGap,
                            parent: Some(parent_ref.clone()),
                            refs: vec![top.reference.clone(), bottom.reference.clone()],
                        },
                    );
                }
            }
        }
    }

    samples
}

fn infer_spacing_families(samples: &[SpacingSample]) -> Vec<SpacingFamily> {
    let mut ordered = samples
        .iter()
        .filter(|sample| finite_positive(sample.value))
        .collect::<Vec<_>>();
    ordered.sort_by(|left, right| left.value.total_cmp(&right.value));

    let mut families = Vec::new();
    let mut current = Vec::<&SpacingSample>::new();
    for sample in ordered {
        if current
            .last()
            .is_some_and(|previous| (sample.value - previous.value).abs() > SPACING_CLUSTER_EPSILON)
        {
            push_spacing_family(&current, &mut families);
            current.clear();
        }
        current.push(sample);
    }
    push_spacing_family(&current, &mut families);
    families
}

fn push_spacing_family(samples: &[&SpacingSample], output: &mut Vec<SpacingFamily>) {
    if samples.len() < 2 {
        return;
    }
    let value = samples.iter().map(|sample| sample.value).sum::<f64>() / samples.len() as f64;
    let mut sources = samples.iter().map(|sample| sample.source).collect::<Vec<_>>();
    sources.sort_by_key(|source| match source {
        SpacingSource::SiblingGap => 0,
        SpacingSource::Padding => 1,
        SpacingSource::EdgeDistance => 2,
    });
    sources.dedup();
    output.push(SpacingFamily {
        value,
        occurrences: samples.len(),
        sources,
    });
}

fn analyze_spacing_outliers(samples: &[SpacingSample], issues: &mut Vec<LayoutIssue>) {
    let mut groups = BTreeMap::<(ElementRef, u8), Vec<&SpacingSample>>::new();
    for sample in samples {
        let Some(parent) = &sample.parent else {
            continue;
        };
        let source = match sample.source {
            SpacingSource::SiblingGap => 0,
            SpacingSource::Padding => 1,
            SpacingSource::EdgeDistance => 2,
        };
        groups.entry((parent.clone(), source)).or_default().push(sample);
    }

    for ((parent, _), group) in groups {
        if group.len() < 3 {
            continue;
        }
        let mut ordered = group.clone();
        ordered.sort_by(|left, right| left.value.total_cmp(&right.value));
        let clusters = cluster_sample_refs(&ordered, SPACING_CLUSTER_EPSILON);
        let Some(family) = clusters.iter().max_by_key(|cluster| cluster.len()) else {
            continue;
        };
        if family.len() < 2 {
            continue;
        }
        let expected = family.iter().map(|sample| sample.value).sum::<f64>() / family.len() as f64;
        let threshold = 2.0_f64.max(expected.abs() * 0.20);
        for sample in ordered {
            let deviation = (sample.value - expected).abs();
            if deviation <= threshold {
                continue;
            }
            let mut refs = vec![parent.clone()];
            refs.extend(sample.refs.iter().cloned());
            refs.sort();
            refs.dedup();
            push_issue(
                issues,
                LayoutIssue {
                    code: "spacing_outlier".into(),
                    severity: Severity::Info,
                    confidence: 0.82,
                    class: LayoutIssueClass::Heuristic,
                    refs,
                    message: "Local spacing measurement deviates from a recurring inferred family".into(),
                    evidence: format!(
                        "inferred_family={expected:.3} measured={:.3} deviation={deviation:.3} threshold={threshold:.3} family_support={}",
                        sample.value,
                        family.len()
                    ),
                },
            );
        }
    }
}

fn analyze_alignment(elements: &[LayoutElement], valid: &[bool], issues: &mut Vec<LayoutIssue>) {
    let groups = children_by_parent(elements, valid);
    for (_parent, children) in groups {
        if children.len() < 3 {
            continue;
        }
        let dimensions: [(&str, fn(&Rect) -> f64); 5] = [
            ("left_edge", |rect| rect.x),
            ("right_edge", rect_right),
            ("horizontal_center", |rect| rect.x + rect.width / 2.0),
            ("top_edge", |rect| rect.y),
            ("bottom_edge", rect_bottom),
        ];
        let mut best = BTreeMap::<ElementRef, AlignmentCandidate>::new();

        for (family_name, measure) in dimensions {
            let mut values = children
                .iter()
                .map(|position| (*position, measure(&elements[*position].rect)))
                .filter(|(_, value)| value.is_finite())
                .collect::<Vec<_>>();
            values.sort_by(|left, right| left.1.total_cmp(&right.1));
            let clusters = cluster_index_values(&values, ALIGNMENT_CLUSTER_EPSILON);
            let Some(family) = clusters.iter().max_by_key(|cluster| cluster.len()) else {
                continue;
            };
            if family.len() < 2 {
                continue;
            }
            let expected = family.iter().map(|(_, value)| *value).sum::<f64>() / family.len() as f64;
            for (position, measured) in &values {
                let deviation = (*measured - expected).abs();
                if deviation <= ALIGNMENT_OUTLIER_THRESHOLD {
                    continue;
                }
                let reference = elements[*position].reference.clone();
                let candidate = AlignmentCandidate {
                    reference: reference.clone(),
                    family: family_name,
                    expected,
                    measured: *measured,
                    deviation,
                    support: family.len(),
                };
                let replace = match best.get(&reference) {
                    None => true,
                    Some(current) => {
                        candidate.support > current.support
                            || (candidate.support == current.support
                                && candidate.deviation < current.deviation)
                    }
                };
                if replace {
                    best.insert(reference, candidate);
                }
            }
        }

        for candidate in best.into_values() {
            let confidence = (0.70 + 0.05 * candidate.support as f32).min(0.92);
            push_issue(
                issues,
                LayoutIssue {
                    code: "alignment_outlier".into(),
                    severity: Severity::Info,
                    confidence,
                    class: LayoutIssueClass::Heuristic,
                    refs: vec![candidate.reference],
                    message: "Element deviates from a local sibling alignment family".into(),
                    evidence: format!(
                        "expected_family={} expected={:.3} measured={:.3} deviation={:.3} threshold={:.3} family_support={}",
                        candidate.family,
                        candidate.expected,
                        candidate.measured,
                        candidate.deviation,
                        ALIGNMENT_OUTLIER_THRESHOLD,
                        candidate.support
                    ),
                },
            );
        }
    }
}

fn children_by_parent(elements: &[LayoutElement], valid: &[bool]) -> BTreeMap<ElementRef, Vec<usize>> {
    let mut groups = BTreeMap::<ElementRef, Vec<usize>>::new();
    for (position, element) in elements.iter().enumerate() {
        if !valid[position] {
            continue;
        }
        if let Some(parent) = &element.parent {
            groups.entry(parent.clone()).or_default().push(position);
        }
    }
    groups
}

fn has_constraining_ancestor(
    elements: &[LayoutElement],
    index: &BTreeMap<ElementRef, usize>,
    element: &LayoutElement,
    x_axis: bool,
) -> bool {
    let mut parent = element.parent.as_ref();
    let mut visited = BTreeSet::new();
    for _ in 0..16 {
        let Some(reference) = parent else {
            return false;
        };
        if !visited.insert(reference.clone()) {
            return false;
        }
        let Some(&position) = index.get(reference) else {
            return false;
        };
        let Some(ancestor) = elements.get(position) else {
            return false;
        };
        let mode = if x_axis {
            ancestor.style.overflow_x
        } else {
            ancestor.style.overflow_y
        };
        if mode.is_some_and(OverflowMode::constrains) {
            return true;
        }
        parent = ancestor.parent.as_ref();
    }
    false
}

fn is_ancestor(
    index: &BTreeMap<ElementRef, usize>,
    elements: &[LayoutElement],
    possible_ancestor: &str,
    descendant: &str,
) -> bool {
    let mut current = index
        .get(descendant)
        .and_then(|position| elements.get(*position))
        .and_then(|element| element.parent.as_ref());
    let mut visited = BTreeSet::new();
    for _ in 0..16 {
        let Some(reference) = current else {
            return false;
        };
        if reference == possible_ancestor {
            return true;
        }
        if !visited.insert(reference.clone()) {
            return false;
        }
        current = index
            .get(reference)
            .and_then(|position| elements.get(*position))
            .and_then(|element| element.parent.as_ref());
    }
    false
}

fn is_fixed_or_sticky(element: &LayoutElement) -> bool {
    matches!(
        element.style.position,
        Some(PositionMode::Fixed) | Some(PositionMode::Sticky)
    )
}

fn occlusion_pair_authority(left: &LayoutElement, right: &LayoutElement) -> bool {
    (left.visibility.sampled
        && left.visibility.occluded == Some(true)
        && left.visibility.occluded_by.as_deref() == Some(right.reference.as_str()))
        || (right.visibility.sampled
            && right.visibility.occluded == Some(true)
            && right.visibility.occluded_by.as_deref() == Some(left.reference.as_str()))
}

fn outside_rect(child: &Rect, parent: &Rect) -> (bool, bool) {
    (
        child.x < parent.x - GEOMETRY_EPSILON
            || rect_right(child) > rect_right(parent) + GEOMETRY_EPSILON,
        child.y < parent.y - GEOMETRY_EPSILON
            || rect_bottom(child) > rect_bottom(parent) + GEOMETRY_EPSILON,
    )
}

fn outside_viewport(rect: &Rect, viewport: (f64, f64)) -> (bool, bool) {
    (
        rect.x < -GEOMETRY_EPSILON || rect_right(rect) > viewport.0 + GEOMETRY_EPSILON,
        rect.y < -GEOMETRY_EPSILON || rect_bottom(rect) > viewport.1 + GEOMETRY_EPSILON,
    )
}

fn meaningful_overlap(left: &Rect, right: &Rect) -> f64 {
    let width = rect_right(left).min(rect_right(right)) - left.x.max(right.x);
    let height = rect_bottom(left).min(rect_bottom(right)) - left.y.max(right.y);
    if width <= 0.0 || height <= 0.0 {
        return 0.0;
    }
    let intersection = width * height;
    let denominator = (left.width * left.height)
        .min(right.width * right.height)
        .max(1.0);
    intersection / denominator
}

fn ranges_overlap(left_start: f64, left_end: f64, right_start: f64, right_end: f64) -> bool {
    left_end > right_start + GEOMETRY_EPSILON && right_end > left_start + GEOMETRY_EPSILON
}

fn rect_right(rect: &Rect) -> f64 {
    rect.x + rect.width
}

fn rect_bottom(rect: &Rect) -> f64 {
    rect.y + rect.height
}

fn push_spacing_sample(samples: &mut Vec<SpacingSample>, sample: SpacingSample) {
    if samples.len() >= MAX_SPACING_SAMPLES || !finite_positive(sample.value) {
        return;
    }
    samples.push(sample);
}

fn cluster_values(values: &[f64], epsilon: f64) -> Vec<Vec<f64>> {
    let mut clusters = Vec::<Vec<f64>>::new();
    for value in values {
        match clusters.last_mut() {
            Some(cluster)
                if cluster
                    .last()
                    .is_some_and(|previous| (*value - *previous).abs() <= epsilon) =>
            {
                cluster.push(*value);
            }
            _ => clusters.push(vec![*value]),
        }
    }
    clusters
}

fn cluster_sample_refs<'a>(
    values: &[&'a SpacingSample],
    epsilon: f64,
) -> Vec<Vec<&'a SpacingSample>> {
    let mut clusters = Vec::<Vec<&SpacingSample>>::new();
    for sample in values {
        match clusters.last_mut() {
            Some(cluster)
                if cluster.last().is_some_and(|previous| {
                    (sample.value - previous.value).abs() <= epsilon
                }) =>
            {
                cluster.push(*sample);
            }
            _ => clusters.push(vec![*sample]),
        }
    }
    clusters
}

fn cluster_index_values(values: &[(usize, f64)], epsilon: f64) -> Vec<Vec<(usize, f64)>> {
    let mut clusters = Vec::<Vec<(usize, f64)>>::new();
    for &(position, value) in values {
        match clusters.last_mut() {
            Some(cluster)
                if cluster
                    .last()
                    .is_some_and(|(_, previous)| (value - *previous).abs() <= epsilon) =>
            {
                cluster.push((position, value));
            }
            _ => clusters.push(vec![(position, value)]),
        }
    }
    clusters
}

fn mean(values: &[f64]) -> f64 {
    values.iter().sum::<f64>() / values.len().max(1) as f64
}

fn finite_positive(value: f64) -> bool {
    value.is_finite() && value > 0.0
}

fn severity_rank(severity: Severity) -> u8 {
    match severity {
        Severity::Info => 1,
        Severity::Warning => 2,
        Severity::Error => 3,
    }
}

fn push_issue(issues: &mut Vec<LayoutIssue>, issue: LayoutIssue) {
    if issues.len() < MAX_LAYOUT_ISSUES {
        issues.push(issue);
    }
}

fn push_fact(facts: &mut Vec<LayoutFact>, fact: LayoutFact) {
    if facts.len() < MAX_LAYOUT_FACTS {
        facts.push(fact);
    }
}
