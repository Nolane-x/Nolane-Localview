use localview_visual::{
    plan_changed_css_regions, ChangedRegionPlan, ChangedRegionPolicy, RgbaImage, VisualError,
};

fn image(width: u32, height: u32) -> RgbaImage {
    RgbaImage {
        width,
        height,
        data: vec![0; (width * height * 4) as usize],
    }
}

fn set_pixel(image: &mut RgbaImage, x: u32, y: u32, value: u8) {
    let offset = ((y * image.width + x) * 4) as usize;
    image.data[offset..offset + 4].copy_from_slice(&[value, value, value, 255]);
}

fn policy(max_regions: usize, fallback_ratio: f64) -> ChangedRegionPolicy {
    ChangedRegionPolicy {
        tile_px: 1,
        threshold: 1,
        max_regions,
        viewport_fallback_ratio: fallback_ratio,
    }
}

#[test]
fn unchanged_frame_schedules_no_regions() {
    let before = image(4, 4);
    let after = before.clone();

    let plan = plan_changed_css_regions(&before, &after, (400.0, 200.0), policy(4, 0.9))
        .expect("unchanged scheduling must succeed");

    assert!(matches!(plan, ChangedRegionPlan::Unchanged));
}

#[test]
fn adjacent_changed_tiles_coalesce_and_map_to_css_space() {
    let before = image(4, 4);
    let mut after = before.clone();
    set_pixel(&mut after, 1, 2, 255);
    set_pixel(&mut after, 2, 2, 255);

    let plan = plan_changed_css_regions(&before, &after, (400.0, 200.0), policy(4, 0.9))
        .expect("bounded changed-region planning must succeed");

    let ChangedRegionPlan::Regions {
        regions,
        changed_ratio,
    } = plan
    else {
        panic!("two adjacent changed pixels should remain a bounded region plan");
    };
    assert_eq!(regions.len(), 1);
    let region = &regions[0];
    assert_eq!(region.x, 100.0);
    assert_eq!(region.y, 100.0);
    assert_eq!(region.width, 200.0);
    assert_eq!(region.height, 50.0);
    assert_eq!(changed_ratio, 2.0 / 16.0);
}

#[test]
fn too_many_disconnected_regions_fall_back_to_viewport() {
    let before = image(5, 5);
    let mut after = before.clone();
    set_pixel(&mut after, 0, 0, 255);
    set_pixel(&mut after, 2, 2, 255);
    set_pixel(&mut after, 4, 4, 255);

    let plan = plan_changed_css_regions(&before, &after, (500.0, 500.0), policy(2, 0.9))
        .expect("fallback planning must succeed");

    let ChangedRegionPlan::Viewport { changed_ratio } = plan else {
        panic!("region budget overflow must fall back to one viewport packet");
    };
    assert_eq!(changed_ratio, 3.0 / 25.0);
}

#[test]
fn broad_visual_change_falls_back_to_viewport_even_when_connected() {
    let before = image(4, 4);
    let mut after = before.clone();
    for y in 0..2 {
        for x in 0..4 {
            set_pixel(&mut after, x, y, 255);
        }
    }

    let plan = plan_changed_css_regions(&before, &after, (400.0, 200.0), policy(8, 0.25))
        .expect("broad-change planning must succeed");

    let ChangedRegionPlan::Viewport { changed_ratio } = plan else {
        panic!("broad visual change should avoid many or oversized region packets");
    };
    assert_eq!(changed_ratio, 0.5);
}

#[test]
fn invalid_policy_or_dimension_mismatch_fails_closed() {
    let before = image(4, 4);
    let after = image(5, 4);
    let mismatch = plan_changed_css_regions(&before, &after, (400.0, 200.0), policy(4, 0.9))
        .expect_err("dimension mismatch must fail closed");
    assert!(matches!(mismatch, VisualError::DimensionMismatch));

    let same = image(4, 4);
    let invalid = plan_changed_css_regions(&same, &same, (400.0, 200.0), policy(0, 0.9))
        .expect_err("zero region budget must be rejected");
    assert!(matches!(invalid, VisualError::InvalidChangePolicy));
}
