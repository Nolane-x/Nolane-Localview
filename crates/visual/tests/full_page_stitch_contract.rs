use localview_visual::{
    FullPageError, RgbaImage, MAX_FULL_PAGE_DOCUMENT_CSS_HEIGHT,
    MAX_FULL_PAGE_OUTPUT_PIXEL_HEIGHT, MAX_FULL_PAGE_OUTPUT_RGBA_BYTES, MAX_FULL_PAGE_TILES,
    plan_full_page, project_full_page_output, stitch_full_page_tile,
};

fn rgba(width: u32, height: u32, rows: &[[u8; 4]]) -> RgbaImage {
    assert_eq!(rows.len(), height as usize);
    let mut data = Vec::with_capacity(width as usize * height as usize * 4);
    for row in rows {
        for _ in 0..width {
            data.extend_from_slice(row);
        }
    }
    RgbaImage { width, height, data }
}

#[test]
fn planner_rejects_non_finite_zero_and_horizontal_drift() {
    for document in [(f64::NAN, 1000.0), (800.0, f64::INFINITY), (0.0, 1000.0)] {
        assert_eq!(
            plan_full_page(document, (800.0, 1000.0), 0.0),
            Err(FullPageError::InvalidGeometry)
        );
    }
    assert_eq!(
        plan_full_page((801.0, 1000.0), (800.0, 1000.0), 0.0),
        Err(FullPageError::HorizontalOverflow)
    );
}

#[test]
fn planner_uses_one_tile_for_short_document_and_zero_normalization() {
    let plan = plan_full_page((800.0, 600.0), (800.0, 1000.0), 200.0).unwrap();
    assert_eq!(plan.scroll_offsets_y, vec![0.0]);
}

#[test]
fn planner_deduplicates_exact_final_offset_and_clamps_partial_bottom() {
    let exact = plan_full_page((800.0, 2000.0), (800.0, 1000.0), 0.0).unwrap();
    assert_eq!(exact.scroll_offsets_y, vec![0.0, 1000.0]);

    let partial = plan_full_page((800.0, 2500.0), (800.0, 1000.0), 0.0).unwrap();
    assert_eq!(partial.scroll_offsets_y, vec![0.0, 1000.0, 1500.0]);
}

#[test]
fn planner_enforces_document_and_tile_bounds() {
    assert_eq!(MAX_FULL_PAGE_TILES, 32);
    assert_eq!(MAX_FULL_PAGE_DOCUMENT_CSS_HEIGHT, 50_000.0);
    assert_eq!(
        plan_full_page(
            (800.0, MAX_FULL_PAGE_DOCUMENT_CSS_HEIGHT + 1.0),
            (800.0, 1000.0),
            0.0,
        ),
        Err(FullPageError::DocumentTooTall)
    );
    assert_eq!(
        plan_full_page((800.0, 33_000.0), (800.0, 1000.0), 0.0),
        Err(FullPageError::TileLimitExceeded)
    );
}

#[test]
fn output_projection_is_fractional_scale_aware_and_bounded() {
    let plan = plan_full_page((4.0, 5.0), (4.0, 2.0), 0.0).unwrap();
    let geometry = project_full_page_output(&plan, (6, 3)).unwrap();
    assert_eq!(geometry.pixel_width, 6);
    assert_eq!(geometry.pixel_height, 8);
    assert_eq!(geometry.rgba_bytes, 6 * 8 * 4);
    assert_eq!(MAX_FULL_PAGE_OUTPUT_PIXEL_HEIGHT, 32_768);
    assert_eq!(MAX_FULL_PAGE_OUTPUT_RGBA_BYTES, 128 * 1024 * 1024);
}

#[test]
fn output_projection_rejects_pixel_height_and_rgba_budget_overflow() {
    let tall = plan_full_page((1.0, 50_000.0), (1.0, 2_000.0), 0.0).unwrap();
    assert_eq!(
        project_full_page_output(&tall, (1, 2_000)),
        Err(FullPageError::OutputPixelHeightExceeded)
    );

    let wide = plan_full_page((20_000.0, 2_000.0), (20_000.0, 2_000.0), 0.0).unwrap();
    assert_eq!(
        project_full_page_output(&wide, (20_000, 2_000)),
        Err(FullPageError::OutputMemoryBudgetExceeded)
    );
}

#[test]
fn stitcher_places_tiles_and_final_overlap_deterministically() {
    let mut output = RgbaImage {
        width: 1,
        height: 4,
        data: vec![0; 16],
    };
    let top = rgba(1, 2, &[[1, 0, 0, 255], [2, 0, 0, 255]]);
    let middle = rgba(1, 2, &[[3, 0, 0, 255], [4, 0, 0, 255]]);
    let bottom = rgba(1, 2, &[[9, 0, 0, 255], [8, 0, 0, 255]]);

    stitch_full_page_tile(&mut output, 2.0, 0.0, &top).unwrap();
    stitch_full_page_tile(&mut output, 2.0, 2.0, &middle).unwrap();
    stitch_full_page_tile(&mut output, 2.0, 2.0, &bottom).unwrap();

    let reds = output.data.chunks_exact(4).map(|px| px[0]).collect::<Vec<_>>();
    assert_eq!(reds, vec![1, 2, 9, 8]);
}

#[test]
fn stitcher_rejects_width_buffer_and_scroll_errors() {
    let tile = rgba(1, 1, &[[1, 2, 3, 255]]);
    let mut wrong_width = RgbaImage { width: 2, height: 1, data: vec![0; 8] };
    assert_eq!(
        stitch_full_page_tile(&mut wrong_width, 1.0, 0.0, &tile),
        Err(FullPageError::DimensionMismatch)
    );

    let mut invalid = RgbaImage { width: 1, height: 1, data: vec![0; 3] };
    assert_eq!(
        stitch_full_page_tile(&mut invalid, 1.0, 0.0, &tile),
        Err(FullPageError::InvalidBuffer)
    );

    let mut output = RgbaImage { width: 1, height: 1, data: vec![0; 4] };
    assert_eq!(
        stitch_full_page_tile(&mut output, 1.0, f64::NAN, &tile),
        Err(FullPageError::InvalidGeometry)
    );
}
