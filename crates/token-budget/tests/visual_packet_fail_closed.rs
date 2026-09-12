use localview_protocol::{DetailLevel, Rect, TokenBudget};
use localview_token_budget::{
    select_visual_packet, VisualPacketBudget, VisualPacketCandidate, VisualPacketSource,
};

fn budget() -> VisualPacketBudget {
    VisualPacketBudget {
        text: TokenBudget {
            max_tokens: 256,
            detail: DetailLevel::Minimal,
        },
        image_regions: 1,
    }
}

fn candidate(confidence_milli: u16) -> VisualPacketCandidate {
    VisualPacketCandidate {
        source: VisualPacketSource::ProgressiveElement,
        rect: Rect {
            x: 10.0,
            y: 10.0,
            width: 80.0,
            height: 40.0,
        },
        information_gain_milli: 900,
        confidence_milli,
        relevance_milli: 1000,
    }
}

#[test]
fn score_outside_normalized_range_fails_closed() {
    let error = select_visual_packet((390, 844), &[candidate(1001)], &budget()).unwrap_err();
    assert_eq!(
        error.to_string(),
        "visual packet candidate scores must be within 0..=1000"
    );
}

#[test]
fn zero_sized_viewport_fails_closed_before_candidate_scoring() {
    let error = select_visual_packet((0, 844), &[candidate(1000)], &budget()).unwrap_err();
    assert_eq!(
        error.to_string(),
        "visual packet viewport dimensions must be positive"
    );
}
