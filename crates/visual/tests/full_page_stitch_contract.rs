use localview_visual::{
    plan_full_page, project_full_page_output, stitch_full_page_tile, FullPageError,
    FullPagePolicy, RgbaImage,
};

fn rgba(width: u32, height: u32, value: u8) -> RgbaImage {
    let len = usize::try_from(width)
        .unwrap()
        .checked_mul(usize::try_from(height).unwrap())
        .and_then(|pixels| pixels.checked_mul(4))
        .unwrap();
    RgbaImage {
        width,
        height,
        data: vec![value; len],
    }
}

#[test]
fn partial_final_tile_uses_exact_max_scroll_y() {
    let plan = plan_full_page(
        800.0,
        1_450.0,
        800.0,
        600.0,
        0.0,
        FullPagePolicy::default(),
    )
    .expect("bounded full-page plan");

    assert_eq!(plan.scroll_offsets_y, vec![0.0, 600.0, 850.0]);
}

#[test]
fn exact_division_does_not_duplicate_final_offset() {
    let plan = plan_full_page(
        800.0,
        1_200.0,
        800.0,
        600.0,
        0.0,
        FullPagePolicy::default(),
    )
    .unwrap();
    assert_eq!(plan.scroll_offsets_y, vec![0.0, 600.0]);
}

#[test]
fn short_document_normalizes_to_zero_scroll() {
    let plan = plan_full_page(
        800.0,
        500.0,
        800.0,
        600.0,
        0.0,
        FullPagePolicy::default(),
    )
    .unwrap();
    assert_eq!(plan.scroll_offsets_y, vec![0.0]);
}

#[test]
fn invalid_geometry_is_rejected() {
    for (document_width, document_height, viewport_width, viewport_height, original_y) in [
        (f64::NAN, 600.0, 800.0, 600.0, 0.0),
        (800.0, f64::INFINITY, 800.0, 600.0, 0.0),
        (800.0, 600.0, 0.0, 600.0, 0.0),
        (800.0, 600.0, 800.0, -1.0, 0.0),
        (800.0, 1_200.0, 800.0, 600.0, f64::NAN),
        (800.0, 1_200.0, 800.0, 600.0, -0.5),
    ] {
        assert_eq!(
            plan_full_page(
                document_width,
                document_height,
                viewport_width,
                viewport_height,
                original_y,
                FullPagePolicy::default(),
            ),
            Err(FullPageError::InvalidGeometry)
        );
    }
}

#[test]
fn horizontal_document_stitching_is_rejected() {
    assert_eq!(
        plan_full_page(
            801.0,
            1_200.0,
            800.0,
            600.0,
            0.0,
            FullPagePolicy::default(),
        ),
        Err(FullPageError::WidthMismatch)
    );
}

#[test]
fn original_scroll_outside_document_is_rejected() {
    assert_eq!(
        plan_full_page(
            800.0,
            1_200.0,
            800.0,
            600.0,
            601.0,
            FullPagePolicy::default(),
        ),
        Err(FullPageError::InvalidGeometry)
    );
}

#[test]
fn document_height_budget_is_enforced() {
    assert_eq!(
        plan_full_page(
            800.0,
            50_001.0,
            800.0,
            600.0,
            0.0,
            FullPagePolicy::default(),
        ),
        Err(FullPageError::DocumentTooTall)
    );
}

#[test]
fn tile_budget_is_enforced() {
    let policy = FullPagePolicy {
        max_tiles: 2,
        ..FullPagePolicy::default()
    };
    assert_eq!(
        plan_full_page(800.0, 1_450.0, 800.0, 600.0, 0.0, policy),
        Err(FullPageError::TileBudgetExceeded)
    );
}

#[test]
fn projected_output_uses_fractional_native_scale() {
    let plan = plan_full_page(
        800.0,
        1_000.0,
        800.0,
        400.0,
        0.0,
        FullPagePolicy::default(),
    )
    .unwrap();
    let output = project_full_page_output(&plan, 1_000, 500, FullPagePolicy::default()).unwrap();
    assert_eq!(output.pixel_width, 1_000);
    assert_eq!(output.pixel_height, 1_250);
    assert_eq!(output.rgba_bytes, 5_000_000);
}

#[test]
fn output_memory_budget_is_enforced_before_allocation() {
    let plan = plan_full_page(
        10.0,
        20.0,
        10.0,
        10.0,
        0.0,
        FullPagePolicy::default(),
    )
    .unwrap();
    let policy = FullPagePolicy {
        max_output_rgba_bytes: 64,
        ..FullPagePolicy::default()
    };
    assert_eq!(
        project_full_page_output(&plan, 10, 10, policy),
        Err(FullPageError::OutputMemoryBudgetExceeded)
    );
}

#[test]
fn output_pixel_height_budget_is_enforced_before_allocation() {
    let plan = plan_full_page(
        10.0,
        20.0,
        10.0,
        10.0,
        0.0,
        FullPagePolicy::default(),
    )
    .unwrap();
    let policy = FullPagePolicy {
        max_output_pixel_height: 15,
        ..FullPagePolicy::default()
    };
    assert_eq!(
        project_full_page_output(&plan, 10, 10, policy),
        Err(FullPageError::OutputPixelHeightExceeded)
    );
}

#[test]
fn output_projection_rejects_checked_arithmetic_overflow() {
    let plan = plan_full_page(1.0, 1.0, 1.0, 1.0, 0.0, FullPagePolicy::default()).unwrap();
    let policy = FullPagePolicy {
        max_output_rgba_bytes: usize::MAX,
        max_output_pixel_height: u32::MAX,
        ..FullPagePolicy::default()
    };
    assert_eq!(
        project_full_page_output(&plan, u32::MAX, u32::MAX, policy),
        Err(FullPageError::ArithmeticOverflow)
    );
}

#[test]
fn overlapping_final_tile_overwrites_only_intersection() {
    let mut output = rgba(1, 5, 0);
    let first = rgba(1, 4, 10);
    let final_tile = rgba(1, 4, 20);

    stitch_full_page_tile(&mut output, 4.0, 0.0, &first).unwrap();
    stitch_full_page_tile(&mut output, 4.0, 2.0, &final_tile).unwrap();

    assert_eq!(&output.data[0..4], &[10, 10, 10, 10]);
    assert_eq!(&output.data[4..8], &[10, 10, 10, 10]);
    assert_eq!(&output.data[8..12], &[20, 20, 20, 20]);
    assert_eq!(&output.data[12..16], &[20, 20, 20, 20]);
    assert_eq!(&output.data[16..20], &[20, 20, 20, 20]);
}

#[test]
fn stitch_rejects_width_mismatch() {
    let mut output = rgba(2, 5, 0);
    let tile = rgba(1, 4, 1);
    assert_eq!(
        stitch_full_page_tile(&mut output, 4.0, 0.0, &tile),
        Err(FullPageError::WidthMismatch)
    );
}

#[test]
fn stitch_rejects_invalid_image_buffer() {
    let mut output = rgba(1, 5, 0);
    let tile = RgbaImage {
        width: 1,
        height: 4,
        data: vec![0; 3],
    };
    assert_eq!(
        stitch_full_page_tile(&mut output, 4.0, 0.0, &tile),
        Err(FullPageError::InvalidImage)
    );
}

#[test]
fn stitch_rejects_out_of_output_placement() {
    let mut output = rgba(1, 5, 0);
    let tile = rgba(1, 4, 1);
    assert_eq!(
        stitch_full_page_tile(&mut output, 4.0, 6.0, &tile),
        Err(FullPageError::PlacementOutOfBounds)
    );
}
