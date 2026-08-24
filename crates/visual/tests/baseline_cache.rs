use std::sync::Arc;

use localview_protocol::SessionId;
use localview_visual::{RgbaImage, VisualBaselineCache, VisualBaselineContext};

fn context(route: &str, css_width: u32, css_height: u32, pixel_width: u32, pixel_height: u32) -> VisualBaselineContext {
    VisualBaselineContext {
        route: route.to_owned(),
        css_width,
        css_height,
        device_scale_factor: 1.0,
        pixel_width,
        pixel_height,
    }
}

fn image(width: u32, height: u32, seed: u8) -> Arc<RgbaImage> {
    Arc::new(RgbaImage {
        width,
        height,
        data: vec![seed; (width * height * 4) as usize],
    })
}

#[test]
fn compatible_baseline_is_reused_without_copying_pixels() {
    let session = SessionId::from_u128(1);
    let ctx = context("http://127.0.0.1:5173/", 2, 2, 2, 2);
    let baseline = image(2, 2, 7);
    let mut cache = VisualBaselineCache::new(64, 4).expect("valid cache policy");

    assert!(cache
        .insert(session, ctx.clone(), baseline.clone())
        .expect("insert baseline"));
    let loaded = cache
        .get_compatible(session, &ctx)
        .expect("compatible baseline must be reusable");

    assert!(Arc::ptr_eq(&loaded, &baseline));
    assert_eq!(cache.len(), 1);
    assert_eq!(cache.used_bytes(), 16);
}

#[test]
fn route_or_viewport_mismatch_invalidates_the_session_baseline() {
    let session = SessionId::from_u128(2);
    let original = context("http://127.0.0.1:5173/", 2, 2, 2, 2);
    let changed_route = context("http://127.0.0.1:5173/settings", 2, 2, 2, 2);
    let mut cache = VisualBaselineCache::new(64, 4).expect("valid cache policy");

    assert!(cache
        .insert(session, original, image(2, 2, 1))
        .expect("insert baseline"));
    assert!(cache.get_compatible(session, &changed_route).is_none());
    assert_eq!(cache.len(), 0);
    assert_eq!(cache.used_bytes(), 0);
}

#[test]
fn global_byte_budget_evicts_the_least_recently_used_session() {
    let first = SessionId::from_u128(10);
    let second = SessionId::from_u128(11);
    let third = SessionId::from_u128(12);
    let ctx = context("http://127.0.0.1:5173/", 2, 2, 2, 2);
    let mut cache = VisualBaselineCache::new(32, 8).expect("valid cache policy");

    assert!(cache
        .insert(first, ctx.clone(), image(2, 2, 1))
        .expect("insert first"));
    assert!(cache
        .insert(second, ctx.clone(), image(2, 2, 2))
        .expect("insert second"));
    assert!(cache.get_compatible(first, &ctx).is_some());
    assert!(cache
        .insert(third, ctx.clone(), image(2, 2, 3))
        .expect("insert third"));

    assert!(cache.get_compatible(first, &ctx).is_some());
    assert!(cache.get_compatible(second, &ctx).is_none());
    assert!(cache.get_compatible(third, &ctx).is_some());
    assert_eq!(cache.used_bytes(), 32);
}

#[test]
fn oversized_replacement_is_not_cached_and_removes_the_stale_entry() {
    let session = SessionId::from_u128(20);
    let small = context("http://127.0.0.1:5173/", 2, 2, 2, 2);
    let large = context("http://127.0.0.1:5173/", 3, 2, 3, 2);
    let mut cache = VisualBaselineCache::new(16, 4).expect("valid cache policy");

    assert!(cache
        .insert(session, small, image(2, 2, 4))
        .expect("insert small baseline"));
    assert!(!cache
        .insert(session, large, image(3, 2, 5))
        .expect("oversized baseline is a bounded miss, not an error"));

    assert_eq!(cache.len(), 0);
    assert_eq!(cache.used_bytes(), 0);
}

#[test]
fn entry_budget_also_evicts_lru_even_when_bytes_fit() {
    let first = SessionId::from_u128(30);
    let second = SessionId::from_u128(31);
    let third = SessionId::from_u128(32);
    let ctx = context("http://127.0.0.1:5173/", 1, 1, 1, 1);
    let mut cache = VisualBaselineCache::new(64, 2).expect("valid cache policy");

    assert!(cache
        .insert(first, ctx.clone(), image(1, 1, 1))
        .expect("insert first"));
    assert!(cache
        .insert(second, ctx.clone(), image(1, 1, 2))
        .expect("insert second"));
    assert!(cache.get_compatible(first, &ctx).is_some());
    assert!(cache
        .insert(third, ctx.clone(), image(1, 1, 3))
        .expect("insert third"));

    assert!(cache.get_compatible(first, &ctx).is_some());
    assert!(cache.get_compatible(second, &ctx).is_none());
    assert!(cache.get_compatible(third, &ctx).is_some());
    assert_eq!(cache.len(), 2);
}
