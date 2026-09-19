use localview_responsive::{
    plan_canonical_sweep, project_contact_sheet, ContactSheetPolicy, ResponsiveError,
    ResponsivePresetId, Viewport,
};

#[test]
fn canonical_presets_are_backend_owned_and_stable() {
    let plan = plan_canonical_sweep(&[
        ResponsivePresetId::Desktop,
        ResponsivePresetId::MobileS,
        ResponsivePresetId::Tablet,
        ResponsivePresetId::Mobile,
    ])
    .expect("valid canonical responsive sweep");

    assert_eq!(
        plan.viewports,
        vec![
            Viewport { width: 320, height: 568 },
            Viewport { width: 390, height: 844 },
            Viewport { width: 768, height: 1024 },
            Viewport { width: 1440, height: 900 },
        ]
    );
    assert_eq!(
        plan.presets,
        vec![
            ResponsivePresetId::MobileS,
            ResponsivePresetId::Mobile,
            ResponsivePresetId::Tablet,
            ResponsivePresetId::Desktop,
        ]
    );
}

#[test]
fn sweep_rejects_empty_and_duplicate_caller_selection() {
    assert_eq!(
        plan_canonical_sweep(&[]).unwrap_err(),
        ResponsiveError::InvalidPresetCount
    );
    assert_eq!(
        plan_canonical_sweep(&[
            ResponsivePresetId::Mobile,
            ResponsivePresetId::Mobile,
        ])
        .unwrap_err(),
        ResponsiveError::DuplicatePreset
    );
}

#[test]
fn contact_sheet_geometry_is_deterministic_and_bounded() {
    let plan = plan_canonical_sweep(&[
        ResponsivePresetId::MobileS,
        ResponsivePresetId::Tablet,
        ResponsivePresetId::Desktop,
    ])
    .unwrap();

    let geometry = project_contact_sheet(
        &plan,
        &[(640, 1136), (1536, 2048), (2880, 1800)],
        ContactSheetPolicy::default(),
    )
    .unwrap();

    assert_eq!(geometry.pixel_width, 2880);
    assert_eq!(geometry.placements.len(), 3);
    assert_eq!(geometry.placements[0].x, 0);
    assert_eq!(geometry.placements[0].y, 0);
    assert_eq!(geometry.placements[0].pixel_width, 640);
    assert_eq!(geometry.placements[0].pixel_height, 1136);
    assert_eq!(
        geometry.placements[1].y,
        1136 + ContactSheetPolicy::default().gutter_px
    );
    assert_eq!(
        geometry.placements[2].y,
        1136 + ContactSheetPolicy::default().gutter_px
            + 2048 + ContactSheetPolicy::default().gutter_px
    );
    assert_eq!(
        geometry.pixel_height,
        1136 + 2048 + 1800 + 2 * ContactSheetPolicy::default().gutter_px
    );
    assert_eq!(
        geometry.rgba_bytes,
        geometry.pixel_width as usize * geometry.pixel_height as usize * 4
    );
    assert!(geometry.rgba_bytes <= ContactSheetPolicy::default().max_rgba_bytes);
}

#[test]
fn contact_sheet_requires_exact_frame_count_and_positive_pixel_dimensions() {
    let plan = plan_canonical_sweep(&[
        ResponsivePresetId::Mobile,
        ResponsivePresetId::Desktop,
    ])
    .unwrap();

    assert_eq!(
        project_contact_sheet(&plan, &[(390, 844)], ContactSheetPolicy::default()).unwrap_err(),
        ResponsiveError::FrameCountMismatch
    );
    assert_eq!(
        project_contact_sheet(
            &plan,
            &[(0, 844), (1440, 900)],
            ContactSheetPolicy::default()
        )
        .unwrap_err(),
        ResponsiveError::InvalidPixelGeometry
    );
}

#[test]
fn contact_sheet_rejects_projected_memory_overflow_before_allocation() {
    let plan = plan_canonical_sweep(&[ResponsivePresetId::Desktop]).unwrap();
    let tiny_policy = ContactSheetPolicy {
        max_rgba_bytes: 1024,
        ..ContactSheetPolicy::default()
    };

    assert_eq!(
        project_contact_sheet(&plan, &[(1440, 900)], tiny_policy).unwrap_err(),
        ResponsiveError::ContactSheetMemoryBudgetExceeded
    );
}
