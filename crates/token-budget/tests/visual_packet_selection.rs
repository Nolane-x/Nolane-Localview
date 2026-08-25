use localview_protocol::{DetailLevel, Rect, TokenBudget};
use localview_token_budget::{
    select_visual_packet, VisualPacketBudget, VisualPacketCandidate, VisualPacketSelectionMode,
    VisualPacketSource,
};

fn rect(x: f64, y: f64, width: f64, height: f64) -> Rect {
    Rect {
        x,
        y,
        width,
        height,
    }
}

fn budget(image_regions: usize) -> VisualPacketBudget {
    VisualPacketBudget {
        text: TokenBudget {
            max_tokens: 500,
            detail: DetailLevel::Normal,
        },
        image_regions,
    }
}

fn candidate(
    source: VisualPacketSource,
    rect: Rect,
    information_gain_milli: u16,
    confidence_milli: u16,
    relevance_milli: u16,
) -> VisualPacketCandidate {
    VisualPacketCandidate {
        source,
        rect,
        information_gain_milli,
        confidence_milli,
        relevance_milli,
    }
}

#[test]
fn local_high_information_evidence_beats_expensive_viewport_fallback() {
    let candidates = vec![
        candidate(
            VisualPacketSource::ViewportFallback,
            rect(0.0, 0.0, 1440.0, 900.0),
            500,
            1000,
            600,
        ),
        candidate(
            VisualPacketSource::ProgressiveElement,
            rect(1000.0, 650.0, 320.0, 180.0),
            900,
            1000,
            1000,
        ),
        candidate(
            VisualPacketSource::ChangedRegion,
            rect(1110.0, 710.0, 170.0, 72.0),
            1000,
            1000,
            1000,
        ),
    ];

    let selected = select_visual_packet((1440, 900), &candidates, &budget(1)).unwrap();

    assert_eq!(selected.mode, VisualPacketSelectionMode::Images);
    assert_eq!(selected.selected.len(), 1);
    assert_eq!(selected.selected[0].source, VisualPacketSource::ChangedRegion);
    assert!(selected.selected[0].normalized_cost_milli < 250);
    assert_eq!(selected.dropped_candidates, 2);
}

#[test]
fn image_region_budget_selects_distinct_changes_without_redundant_nested_context() {
    let candidates = vec![
        candidate(
            VisualPacketSource::ChangedRegion,
            rect(40.0, 40.0, 100.0, 60.0),
            1000,
            1000,
            1000,
        ),
        candidate(
            VisualPacketSource::ProgressiveElement,
            rect(0.0, 0.0, 220.0, 180.0),
            900,
            1000,
            1000,
        ),
        candidate(
            VisualPacketSource::ChangedRegion,
            rect(1000.0, 700.0, 120.0, 80.0),
            1000,
            1000,
            900,
        ),
        candidate(
            VisualPacketSource::ViewportFallback,
            rect(0.0, 0.0, 1440.0, 900.0),
            500,
            1000,
            500,
        ),
    ];

    let selected = select_visual_packet((1440, 900), &candidates, &budget(2)).unwrap();

    assert_eq!(selected.selected.len(), 2);
    assert!(selected
        .selected
        .iter()
        .all(|item| item.source == VisualPacketSource::ChangedRegion));
    assert!(selected.selected[0].rect.x < selected.selected[1].rect.x);
}

#[test]
fn zero_image_budget_is_explicit_metadata_only_instead_of_silent_viewport_widening() {
    let candidates = vec![candidate(
        VisualPacketSource::ProgressiveElement,
        rect(50.0, 50.0, 120.0, 44.0),
        900,
        1000,
        1000,
    )];

    let selected = select_visual_packet((390, 844), &candidates, &budget(0)).unwrap();

    assert_eq!(selected.mode, VisualPacketSelectionMode::MetadataOnly);
    assert!(selected.selected.is_empty());
    assert_eq!(selected.dropped_candidates, 1);
}

#[test]
fn invalid_or_out_of_viewport_candidate_geometry_fails_closed() {
    let candidates = vec![candidate(
        VisualPacketSource::ChangedRegion,
        rect(380.0, 800.0, 40.0, 80.0),
        1000,
        1000,
        1000,
    )];

    let error = select_visual_packet((390, 844), &candidates, &budget(1)).unwrap_err();
    assert_eq!(error.to_string(), "visual packet candidate geometry is outside the viewport");
}

#[test]
fn selection_is_input_order_independent_for_equal_candidate_sets() {
    let left = candidate(
        VisualPacketSource::ChangedRegion,
        rect(40.0, 40.0, 80.0, 80.0),
        1000,
        1000,
        1000,
    );
    let right = candidate(
        VisualPacketSource::ChangedRegion,
        rect(800.0, 40.0, 80.0, 80.0),
        1000,
        1000,
        1000,
    );

    let forward = select_visual_packet((1200, 800), &[left.clone(), right.clone()], &budget(2))
        .unwrap();
    let reversed = select_visual_packet((1200, 800), &[right, left], &budget(2)).unwrap();

    assert_eq!(forward.selected, reversed.selected);
}
