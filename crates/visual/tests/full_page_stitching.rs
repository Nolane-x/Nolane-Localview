use localview_visual::{
    plan_full_page, project_output_height_px, scroll_tolerance_css, stitch_full_page_tile,
    FullPagePlanError, FullPagePolicy, FullPageStitchError, RgbaImage,
};

fn policy() -> FullPagePolicy {
    FullPagePolicy {
        max_tiles: 32,
        max_document_css_height: 50_000.0,
        max_output_rgba_bytes: 128 * 1024 * 1024,
        max_output_pixel_height: 32_768,
    }
}

fn rgba(width: u32, height: u32, value: u8) -> RgbaImage {
    RgbaImage {
        width,
        height,
        data: vec![value; width as usize * height as usize * 4],
    }
}

#[test]
fn planner_single_viewport_page_uses_zero_origin() {
    let plan = plan_full_page(1200.0, 800.0, 1200.0, 800.0, 0.0, policy()).unwrap();
    assert_eq!(plan.scroll_offsets_y, vec![0.0]);
}

#[test]
fn planner_adds_exact_bottom_clamped_tile_without_duplicate_offset() {
    let plan = plan_full_page(1200.0, 2500.0, 1200.0, 1000.0, 0.0, policy()).unwrap();
    assert_eq!(plan.scroll_offsets_y, vec![0.0, 1000.0, 1500.0]);
}

#[test]
fn planner_exact_divisibility_does_not_duplicate_final_offset() {
    let plan = plan_full_page(1200.0, 3000.0, 1200.0, 1000.0, 0.0, policy()).unwrap();
    assert_eq!(plan.scroll_offsets_y, vec![0.0, 1000.0, 2000.0]);
}

#[test]
fn planner_rejects_document_wider_than_viewport_authority() {
    let err = plan_full_page(1400.0, 2000.0, 1200.0, 1000.0, 0.0, policy()).unwrap_err();
    assert!(matches!(err, FullPagePlanError::HorizontalStitchingUnsupported));
}

#[test]
fn planner_rejects_non_finite_geometry() {
    let err = plan_full_page(1200.0, f64::INFINITY, 1200.0, 1000.0, 0.0, policy()).unwrap_err();
    assert!(matches!(err, FullPagePlanError::InvalidGeometry));
}

#[test]
fn planner_rejects_document_height_over_policy() {
    let err = plan_full_page(1200.0, 50_001.0, 1200.0, 1000.0, 0.0, policy()).unwrap_err();
    assert!(matches!(err, FullPagePlanError::DocumentTooTall));
}

#[test]
fn planner_rejects_more_than_32_tiles() {
    let err = plan_full_page(1000.0, 33_000.0, 1000.0, 1000.0, 0.0, policy()).unwrap_err();
    assert!(matches!(err, FullPagePlanError::TooManyTiles));
}

#[test]
fn projected_height_supports_fractional_native_scale_without_integer_css_assumption() {
    let height = project_output_height_px(2500.0, 1000.0, 1200, 1500, policy()).unwrap();
    assert_eq!(height, 3750);
}

#[test]
fn projected_height_rejects_output_over_pixel_height_budget() {
    let err = project_output_height_px(50_000.0, 1000.0, 1000, 1000, policy()).unwrap_err();
    assert!(matches!(err, FullPagePlanError::OutputTooTall));
}

#[test]
fn projected_height_rejects_output_over_rgba_budget() {
    let err = project_output_height_px(
        1000.0,
        1000.0,
        1000,
        1000,
        FullPagePolicy {
            max_tiles: 64,
            max_document_css_height: 50_000.0,
            max_output_rgba_bytes: 1024,
            max_output_pixel_height: 32_768,
        },
    )
    .unwrap_err();
    assert!(matches!(err, FullPagePlanError::OutputTooLarge));
}

#[test]
fn scroll_tolerance_is_one_native_pixel_with_quarter_css_floor() {
    assert_eq!(scroll_tolerance_css(2.0).unwrap(), 0.5);
    assert_eq!(scroll_tolerance_css(8.0).unwrap(), 0.25);
}

#[test]
fn scroll_tolerance_rejects_non_positive_or_non_finite_scale() {
    assert!(matches!(
        scroll_tolerance_css(0.0),
        Err(FullPagePlanError::InvalidScale)
    ));
    assert!(matches!(
        scroll_tolerance_css(f64::NAN),
        Err(FullPagePlanError::InvalidScale)
    ));
}

#[test]
fn stitcher_places_tile_from_acknowledged_fractional_scale_scroll() {
    let mut output = rgba(4, 6, 0);
    let tile = rgba(4, 2, 7);
    stitch_full_page_tile(&mut output, 2.0, 1.5, &tile).unwrap();
    let first_written_row = 3usize * 4usize * 4usize;
    assert!(output.data[first_written_row..first_written_row + 16]
        .iter()
        .all(|byte| *byte == 7));
}

#[test]
fn stitcher_overwrites_only_overlapping_rows_for_final_clamped_tile() {
    let mut output = rgba(2, 5, 0);
    let first = rgba(2, 3, 1);
    let final_tile = rgba(2, 3, 9);
    stitch_full_page_tile(&mut output, 0.0, 1.0, &first).unwrap();
    stitch_full_page_tile(&mut output, 2.0, 1.0, &final_tile).unwrap();
    let row2 = 2usize * 2usize * 4usize;
    assert!(output.data[row2..row2 + 8].iter().all(|byte| *byte == 9));
}

#[test]
fn stitcher_rejects_width_mismatch() {
    let mut output = rgba(4, 6, 0);
    let tile = rgba(3, 2, 7);
    let err = stitch_full_page_tile(&mut output, 0.0, 1.0, &tile).unwrap_err();
    assert!(matches!(err, FullPageStitchError::WidthMismatch));
}

#[test]
fn stitcher_rejects_non_finite_or_negative_scroll() {
    let mut output = rgba(4, 6, 0);
    let tile = rgba(4, 2, 7);
    assert!(matches!(
        stitch_full_page_tile(&mut output, -1.0, 1.0, &tile),
        Err(FullPageStitchError::InvalidPlacement)
    ));
    assert!(matches!(
        stitch_full_page_tile(&mut output, f64::NAN, 1.0, &tile),
        Err(FullPageStitchError::InvalidPlacement)
    ));
}

#[test]
fn stitcher_rejects_tile_start_beyond_output() {
    let mut output = rgba(4, 6, 0);
    let tile = rgba(4, 2, 7);
    let err = stitch_full_page_tile(&mut output, 7.0, 1.0, &tile).unwrap_err();
    assert!(matches!(err, FullPageStitchError::OutOfBounds));
}
