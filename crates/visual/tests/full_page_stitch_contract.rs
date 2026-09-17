use localview_visual::{plan_full_page, FullPagePolicy};

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
