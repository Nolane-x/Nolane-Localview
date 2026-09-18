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
    assert!(i18n.contains("type Dictionary = Record<MessageKey, string>;"));
    assert!(!i18n.contains("Partial<Record<MessageKey, string>>"));

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
    assert!(inspector.contains("translate(locale, 'action.openSource')"));
    assert!(inspector.contains("translate(locale, 'action.measure')"));
    assert!(inspector.contains("translate(locale, 'action.capture')"));
    assert!(inspector.contains("translate(locale, 'action.askAi')"));

    let advanced = between(source, "function AdvancedPanel(", "function ResponsivePanel(");
    assert!(advanced.contains("Project identity"));
    assert!(advanced.contains("EvidenceCard"));
    assert!(advanced.contains("observer"));
}

#[test]
fn inspector_never_presents_unwired_primary_actions_as_enabled() {
    let source = include_str!("../../src/features/FloatingTools.tsx");
    let inspector = between(source, "function Inspector(", "function AdvancedPanel(");

    assert!(inspector.contains("function UnavailableInspectorAction("));
    for key in [
        "action.openSource",
        "action.measure",
        "action.capture",
        "action.askAi",
        "action.fix",
    ] {
        assert!(
            inspector.contains(&format!("translate(locale, '{key}')")),
            "missing localized unavailable action for {key}"
        );
    }
    assert!(inspector.contains("disabled"));
    assert!(inspector.contains("aria-disabled=\"true\""));
    assert!(!inspector.contains("<button><CaptureIcon"));
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
    let shell = include_str!("../../src/app/LocalViewShell.tsx");

    assert!(tools.contains("| 'settings'"));
    assert!(tools.contains("| 'advanced'"));
    assert!(tools.contains("function SettingsPanel("));
    assert!(tools.contains("function AdvancedPanel("));
    let settings = between(tools, "function SettingsPanel(", "function ResponsivePanel(");
    assert!(settings.contains("translate(locale, 'settings.language')"));
    assert!(settings.contains("translate(locale, 'settings.showTargetBar')"));
    assert!(settings.contains("translate(locale, 'settings.showToolRail')"));
    assert!(!settings.contains("settings.rememberChrome"));
    assert!(!settings.contains("rememberChromePositions"));
    assert!(shell.contains("<RailButton tool=\"settings\""));
    assert!(shell.contains("<SettingsIcon/>"));
}




#[test]
fn command_palette_search_is_functional_not_decorative() {
    let source = include_str!("../../src/features/FloatingTools.tsx");
    let command = between(source, "function CommandPanel(", "function ConsoleRow(");

    assert!(source.contains("useState"));
    assert!(command.contains("const [query, setQuery] = useState('')"));
    assert!(command.contains("const visibleCommands = commands.filter"));
    assert!(command.contains("value={query}"));
    assert!(command.contains("onChange={(event) => setQuery(event.target.value)}"));
    assert!(command.contains("visibleCommands.map"));
}

#[test]
fn human_first_panels_do_not_enable_unwired_responsive_or_ai_actions() {
    let source = include_str!("../../src/features/FloatingTools.tsx");
    let responsive = between(source, "function ResponsivePanel(", "function ConsolePanel(");
    let ai = between(source, "function AiPanel(", "function SessionsPanel(");

    assert!(!responsive.contains("disabled={!current}"));
    assert!(responsive.contains("disabled aria-disabled=\"true\""));
    assert!(!ai.contains("disabled={!current}"));
    assert!(ai.matches("disabled aria-disabled=\"true\"").count() >= 4);
    assert!(ai.contains("translate(locale, 'ai.unavailable')"));
}

#[test]
fn primary_tool_rail_uses_active_locale() {
    let shell = include_str!("../../src/app/LocalViewShell.tsx");
    let tools = include_str!("../../src/features/FloatingTools.tsx");

    assert!(shell.contains("locale={preferences.locale}"));
    assert!(shell.contains("locale={locale}"));
    for key in [
        "tool.inspect",
        "tool.responsive",
        "tool.console",
        "tool.network",
        "tool.ai",
        "tool.settings",
        "tool.advanced",
        "tool.command",
    ] {
        assert!(
            tools.contains(&format!("'{key}'")),
            "tool rail is missing localization key {key}"
        );
    }
    assert!(tools.contains("translate(locale, meta.messageKey)"));
    assert!(tools.contains("translate(locale, 'tool.command')"));
}



#[test]
fn immersive_chrome_recovers_for_keyboard_focus() {
    let styles = include_str!("../../src/styles.css");

    assert!(styles.contains(".is-immersive .top-pill:focus-within"));
    assert!(styles.contains(".is-immersive .floating-rail:focus-within"));
    assert!(styles.contains("button:focus-visible"));
}

#[test]
fn render_audit_covers_minimum_human_first_states() {
    let capture = include_str!("../../../../tools/human-first-ui-v2/capture.mjs");

    for artifact in [
        "02-en-inspector.png",
        "11-en-inspector-no-selection.png",
        "12-tool-rail-hidden.png",
        "13-tool-rail-restored.png",
        "14-no-target.png",
        "15-ai-unavailable.png",
    ] {
        assert!(
            capture.contains(artifact),
            "render audit is missing required state artifact {artifact}"
        );
    }
    assert!(capture.contains("Show tool rail"));
    assert!(capture.contains("liveNoFocus"));
    assert!(capture.contains("dashboardNoTarget"));
}

#[test]
fn visual_system_uses_muted_moss_instead_of_ai_blue() {
    let styles = include_str!("../../src/styles.css");

    assert!(styles.contains("--lv-accent:#9aa982"));
    assert!(styles.contains("--lv-accent-soft"));
    assert!(!styles.contains("--lv-accent:#8bb6ff"));
}
