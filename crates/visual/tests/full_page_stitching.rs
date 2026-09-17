use localview_visual::{
    plan_full_page, MAX_FULL_PAGE_DOCUMENT_CSS_HEIGHT, MAX_FULL_PAGE_TILES,
};

fn assert_close(actual: f64, expected: f64) {
    assert!(
        (actual - expected).abs() <= 1e-9,
        "expected {expected}, got {actual}"
    );
}

#[test]
fn shorter_than_viewport_is_one_cropped_contribution() {
    let plan = plan_full_page((800.0, 600.0), (800.0, 1000.0)).unwrap();
    assert_eq!(plan.tiles.len(), 1);
    let tile = &plan.tiles[0];
    assert_close(tile.scroll_y_css, 0.0);
    assert_close(tile.source_start_y_css, 0.0);
    assert_close(tile.destination_start_y_css, 0.0);
    assert_close(tile.contribution_height_css, 600.0);
}

#[test]
fn exact_viewport_multiple_has_non_overlapping_tiles() {
    let plan = plan_full_page((800.0, 2000.0), (800.0, 1000.0)).unwrap();
    assert_eq!(plan.tiles.len(), 2);
    assert_close(plan.tiles[0].scroll_y_css, 0.0);
    assert_close(plan.tiles[0].source_start_y_css, 0.0);
    assert_close(plan.tiles[0].destination_start_y_css, 0.0);
    assert_close(plan.tiles[0].contribution_height_css, 1000.0);
    assert_close(plan.tiles[1].scroll_y_css, 1000.0);
    assert_close(plan.tiles[1].source_start_y_css, 0.0);
    assert_close(plan.tiles[1].destination_start_y_css, 1000.0);
    assert_close(plan.tiles[1].contribution_height_css, 1000.0);
}

#[test]
fn clamped_final_scroll_overlaps_but_contributes_each_row_once() {
    let plan = plan_full_page((800.0, 2500.0), (800.0, 1000.0)).unwrap();
    assert_eq!(plan.tiles.len(), 3);

    assert_close(plan.tiles[0].scroll_y_css, 0.0);
    assert_close(plan.tiles[1].scroll_y_css, 1000.0);
    assert_close(plan.tiles[2].scroll_y_css, 1500.0);

    assert_close(plan.tiles[2].source_start_y_css, 500.0);
    assert_close(plan.tiles[2].destination_start_y_css, 2000.0);
    assert_close(plan.tiles[2].contribution_height_css, 500.0);

    let covered: f64 = plan
        .tiles
        .iter()
        .map(|tile| tile.contribution_height_css)
        .sum();
    assert_close(covered, 2500.0);
}

#[test]
fn rejects_horizontal_document_overflow() {
    assert!(plan_full_page((1001.0, 1000.0), (1000.0, 1000.0)).is_err());
    assert!(plan_full_page((1000.5, 1000.0), (1000.0, 1000.0)).is_ok());
}

#[test]
fn rejects_non_finite_or_non_positive_geometry() {
    for document in [
        (f64::NAN, 1000.0),
        (1000.0, f64::INFINITY),
        (0.0, 1000.0),
        (1000.0, 0.0),
    ] {
        assert!(plan_full_page(document, (1000.0, 1000.0)).is_err());
    }
    assert!(plan_full_page((1000.0, 1000.0), (0.0, 1000.0)).is_err());
    assert!(plan_full_page((1000.0, 1000.0), (1000.0, f64::NAN)).is_err());
}

#[test]
fn rejects_document_height_above_bound() {
    assert!(plan_full_page(
        (1000.0, MAX_FULL_PAGE_DOCUMENT_CSS_HEIGHT + 1.0),
        (1000.0, 1000.0)
    )
    .is_err());
}

#[test]
fn rejects_plan_requiring_more_than_tile_cap() {
    let viewport_height = 1.0;
    let document_height = (MAX_FULL_PAGE_TILES as f64) + 1.0;
    assert!(plan_full_page(
        (1000.0, document_height),
        (1000.0, viewport_height)
    )
    .is_err());
}
