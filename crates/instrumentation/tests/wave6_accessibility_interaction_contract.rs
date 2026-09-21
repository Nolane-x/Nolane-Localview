use localview_instrumentation::wave6::wave6_bootstrap_script;

#[test]
fn wave6_bootstrap_is_local_bounded_and_separate_from_visual_capture() {
    let script = wave6_bootstrap_script();
    assert!(script.contains("__LOCALVIEW_WAVE6__"));
    assert!(script.contains("runAccessibilityScan"));
    assert!(script.contains("beginKeyboardJourney"));
    assert!(script.contains("effectiveHitbox"));
    assert!(script.contains("beginFeedbackProbe"));
    assert!(script.contains("validateReplayStep"));
    assert!(script.contains("data-localview-owned"));
    assert!(script.contains("data-localview-visual-freeze"));
    assert!(!script.contains("fetch('http"));
    assert!(!script.contains("document.cookie"));
}
