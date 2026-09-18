fn between<'a>(source: &'a str, start: &str, end: &str) -> &'a str {
    let start_index = source.find(start).expect("start marker must exist");
    let tail = &source[start_index..];
    let end_index = tail.find(end).expect("end marker must exist after start");
    &tail[..end_index]
}

#[test]
fn human_first_ui_v2_has_persisted_settings_and_localization_foundation() {
    let i18n = include_str!("../../src/i18n.ts");
    let preferences = include_str!("../../src/preferences.ts");

    assert!(i18n.contains("export const DEFAULT_LOCALE = 'en'"));
    for locale in [
        "'en'", "'vi'", "'zh-CN'", "'zh-TW'", "'ja'", "'ko'",
        "'es'", "'fr'", "'de'", "'pt-BR'", "'id'", "'th'",
    ] {
        assert!(i18n.contains(locale), "missing locale {locale}");
    }
    assert!(i18n.contains("fallback"));
    assert!(i18n.contains("document.documentElement.lang"));

    assert!(preferences.contains("showTargetBar: true"));
    assert!(preferences.contains("showToolRail: true"));
    assert!(preferences.contains("locale: DEFAULT_LOCALE"));
    assert!(preferences.contains("localStorage"));
    assert!(preferences.contains("resetWorkspace"));
}

#[test]
fn default_human_inspector_hides_machine_diagnostics() {
    let source = include_str!("../../src/features/FloatingTools.tsx");
    let inspector = between(source, "function Inspector(", "function AdvancedPanel(");

    assert!(!inspector.contains("Semantic Snapshot"));
    assert!(!inspector.contains("Project identity"));
    assert!(!inspector.contains("X-Ray pipeline"));
    assert!(!inspector.contains("<EvidenceCard"));
    assert!(inspector.contains("Open source"));
    assert!(inspector.contains("Measure"));
    assert!(inspector.contains("Capture"));
    assert!(inspector.contains("Ask AI"));

    let advanced = between(source, "function AdvancedPanel(", "function ResponsivePanel(");
    assert!(advanced.contains("Project identity"));
    assert!(advanced.contains("EvidenceCard"));
    assert!(advanced.contains("observer"));
}

#[test]
fn top_target_bar_is_hideable_and_human_facing() {
    let shell = include_str!("../../src/app/LocalViewShell.tsx");

    assert!(shell.contains("preferences.showTargetBar"));
    assert!(shell.contains("onHideTargetBar"));
    assert!(shell.contains("workspace.targetBar.toggle"));
    assert!(shell.contains("Ctrl+Shift+T"));
    assert!(!shell.contains("observer idle"));
    assert!(!shell.contains("Native observer attached"));
}

#[test]
fn settings_and_advanced_are_real_tool_surfaces() {
    let tools = include_str!("../../src/features/FloatingTools.tsx");

    assert!(tools.contains("| 'settings'"));
    assert!(tools.contains("| 'advanced'"));
    assert!(tools.contains("function SettingsPanel("));
    assert!(tools.contains("function AdvancedPanel("));
    assert!(tools.contains("Language"));
    assert!(tools.contains("Show target bar"));
    assert!(tools.contains("Show tool rail"));
}

#[test]
fn visual_system_uses_muted_moss_instead_of_ai_blue() {
    let styles = include_str!("../../src/styles.css");

    assert!(styles.contains("--lv-accent:#9aa982"));
    assert!(styles.contains("--lv-accent-soft"));
    assert!(!styles.contains("--lv-accent:#8bb6ff"));
}
