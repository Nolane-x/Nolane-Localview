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
    assert!(shell.contains("COMMAND_IDS.workspaceToggleTargetBar"));
    let commands = include_str!("../../src/commands.ts");
    assert!(commands.contains("workspaceToggleTargetBar: 'workspace.targetBar.toggle'"));
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
        "22-vi-no-target.png",
        "23-vi-disconnected.png",
        "24-vi-inspector-accessibility.png",
        "25-vi-console.png",
        "26-vi-network.png",
        "27-vi-live-inspection-unavailable.png",
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
fn render_audit_is_executable_not_screenshot_only() {
    let capture = include_str!("../../../../tools/human-first-ui-v2/capture.mjs");

    for marker in [
        "assertNoHorizontalOverflow",
        "assertVisibleButtonsNamed",
        "assertVisible",
        "assertHidden",
        "assertDocumentLocale",
        "audit.json",
        "malformed-preferences-recovered",
    ] {
        assert!(
            capture.contains(marker),
            "render audit is missing executable invariant marker {marker}"
        );
    }

    assert!(capture.contains("document.documentElement.scrollWidth"));
    assert!(capture.contains("localview.preferences.v2"));
    assert!(capture.contains("JSON.stringify(audit"));
}



#[test]
fn primary_target_bar_and_runtime_error_copy_are_localized_and_humanized() {
    let shell = include_str!("../../src/app/LocalViewShell.tsx");
    let i18n = include_str!("../../src/i18n.ts");
    let runtime = between(shell, "function RuntimeToast(", "function IconButton(");

    for key in [
        "action.showSessions",
        "action.immersive",
        "action.retry",
        "aria.currentSession",
        "runtime.unavailable",
        "runtime.unavailableHint",
    ] {
        assert!(
            i18n.contains(&format!("'{key}'")),
            "missing primary chrome localization key {key}"
        );
    }

    assert!(shell.contains("translate(locale, 'action.showSessions')"));
    assert!(shell.contains("translate(locale, 'aria.currentSession')"));
    assert!(shell.contains("translate(locale, 'action.immersive')"));
    assert!(runtime.contains("translate(locale, 'runtime.unavailable')"));
    assert!(runtime.contains("translate(locale, 'runtime.unavailableHint')"));
    assert!(runtime.contains("translate(locale, 'action.retry')"));
    assert!(
        !runtime.contains("{error}"),
        "primary runtime toast must not expose raw internal exception text"
    );
}



#[test]
fn top_level_human_panels_do_not_split_language() {
    let tools = include_str!("../../src/features/FloatingTools.tsx");
    let i18n = include_str!("../../src/i18n.ts");

    for key in [
        "sessions.title",
        "sessions.detectedOne",
        "sessions.detectedMany",
        "responsive.viewports",
        "responsive.unavailable",
        "responsive.note",
        "command.searchPlaceholder",
        "command.searchAria",
    ] {
        assert!(
            i18n.contains(&format!("'{key}'")),
            "missing top-level human-panel localization key {key}"
        );
    }

    let responsive = between(tools, "function ResponsivePanel(", "function ConsolePanel(");
    assert!(responsive.contains("locale: SupportedLocale"));
    assert!(responsive.contains("translate(locale, 'responsive.viewports')"));
    assert!(responsive.contains("translate(locale, 'responsive.unavailable')"));
    assert!(responsive.contains("translate(locale, 'responsive.note')"));
    assert!(responsive.contains("translate(locale, 'empty.noTarget')"));
    assert!(!responsive.contains(">VIEWPORTS<"));
    assert!(!responsive.contains("Viewport tools open only when needed."));

    let sessions = between(tools, "function SessionsPanel(", "function CommandPanel(");
    assert!(sessions.contains("locale: SupportedLocale"));
    assert!(sessions.contains("translate(locale, 'sessions.detectedOne')"));
    assert!(sessions.contains("translate(locale, 'sessions.detectedMany')"));
    assert!(sessions.contains("translate(locale, 'empty.noTarget')"));
    assert!(sessions.contains("translate(locale, 'empty.runDevServer')"));

    let command = between(tools, "function CommandPanel(", "function ConsoleRow(");
    assert!(command.contains("translate(locale, 'command.searchPlaceholder')"));
    assert!(command.contains("translate(locale, 'command.searchAria')"));
    assert!(!command.contains("placeholder=\"Type a command…\""));
    assert!(!command.contains("aria-label=\"Search commands\""));

    assert!(tools.contains("sessions: translate(locale, 'sessions.title')"));
    assert!(tools.contains("translate(locale, 'action.close')"));
}



#[test]
fn command_palette_routes_through_canonical_command_ids() {
    let shell = include_str!("../../src/app/LocalViewShell.tsx");
    let tools = include_str!("../../src/features/FloatingTools.tsx");
    let commands = include_str!("../../src/commands.ts");

    assert!(commands.contains("aiOpen: 'ai.open'"));
    assert!(tools.contains("import { COMMAND_IDS, type CommandId } from '../commands';"));
    assert!(tools.contains("onCommand: (command: CommandId) => void"));
    assert!(tools.contains("key={command.id}"));

    for id in [
        "COMMAND_IDS.inspectActivate",
        "COMMAND_IDS.responsiveOpen",
        "COMMAND_IDS.consoleOpen",
        "COMMAND_IDS.networkOpen",
        "COMMAND_IDS.aiOpen",
        "COMMAND_IDS.previewOpenNative",
        "COMMAND_IDS.settingsOpen",
        "COMMAND_IDS.advancedOpen",
        "COMMAND_IDS.workspaceToggleTargetBar",
        "COMMAND_IDS.workspaceToggleToolRail",
        "COMMAND_IDS.sessionPauseDiscovery",
    ] {
        assert!(
            tools.contains(id),
            "command palette is missing canonical command id {id}"
        );
    }

    assert!(shell.contains("type CommandId"));
    assert!(shell.contains("const executeCommand = useCallback("));
    assert!(shell.contains("switch (command)"));
    assert!(shell.contains("case COMMAND_IDS.inspectActivate:"));
    assert!(shell.contains("case COMMAND_IDS.workspaceToggleTargetBar:"));
    assert!(shell.contains("case COMMAND_IDS.workspaceToggleToolRail:"));
    assert!(shell.contains("case COMMAND_IDS.sessionPauseDiscovery:"));
    assert!(shell.contains("onCommand={executeCommand}"));
    assert!(!tools.contains("key={command.title}"));
}



#[test]
fn explicit_reduced_motion_preference_is_respected() {
    let shell = include_str!("../../src/app/LocalViewShell.tsx");
    let styles = include_str!("../../src/styles.css");

    assert!(shell.contains("preferences.reducedMotion === 'reduce'"));
    assert!(shell.contains("is-reduced-motion"));
    assert!(styles.contains(".is-reduced-motion *"));
    assert!(styles.contains("animation-duration:.001ms!important"));
    assert!(styles.contains("transition-duration:.001ms!important"));
}

#[test]
fn render_audit_exercises_preference_corruption_and_legacy_recovery() {
    let capture = include_str!("../../../../tools/human-first-ui-v2/capture.mjs");

    for marker in [
        "invalid-preferences-normalized",
        "partial-preferences-recovered",
        "explicit-reduced-motion",
        "readStoredPreferences",
        "targetBarPosition",
        "toolRailPosition",
        "1e309",
        "annotationPersistence",
        "notifications",
        "autoOpen",
        "density",
        "accent",
    ] {
        assert!(
            capture.contains(marker),
            "render audit is missing preference recovery marker {marker}"
        );
    }
}




#[test]
fn workspace_empty_state_uses_active_locale() {
    let surface = include_str!("../../src/app/WorkspaceSurface.tsx");
    let shell = include_str!("../../src/app/LocalViewShell.tsx");

    assert!(surface.contains("type SupportedLocale"));
    assert!(surface.contains("locale: SupportedLocale"));
    assert!(surface.contains("translate(locale, 'empty.noTarget')"));
    assert!(surface.contains("translate(locale, 'empty.runDevServer')"));
    assert!(surface.contains("translate(locale, 'tool.command')"));
    assert!(shell.contains("locale={preferences.locale}"));
    assert!(!surface.contains("Your localhost becomes the workspace."));
    assert!(!surface.contains("Open command palette"));
}




#[test]
fn console_network_and_live_inspection_notice_use_active_locale() {
    let tools = include_str!("../../src/features/FloatingTools.tsx");
    let i18n = include_str!("../../src/i18n.ts");
    let capture = include_str!("../../../../tools/human-first-ui-v2/capture.mjs");

    for key in [
        "console.live",
        "console.eventOne",
        "console.eventMany",
        "console.emptyTitle",
        "console.emptyText",
        "network.target",
        "network.requests",
        "network.failures",
        "network.emptyTitle",
        "network.emptyText",
        "observer.connectTitle",
        "observer.connectText",
    ] {
        assert!(i18n.contains(&format!("'{key}'")), "missing localized primary-panel key {key}");
    }

    let console = between(tools, "function ConsolePanel(", "function NetworkPanel(");
    assert!(console.contains("translate(locale, 'console.live')"));
    assert!(console.contains("console.eventOne"));
    assert!(console.contains("console.eventMany"));
    assert!(!console.contains(">Live<"));
    assert!(!console.contains("No console events"));

    let network = between(tools, "function NetworkPanel(", "function AiPanel(");
    assert!(network.contains("translate(locale, 'network.target')"));
    assert!(network.contains("translate(locale, 'network.requests')"));
    assert!(network.contains("translate(locale, 'network.failures')"));
    assert!(!network.contains("<span>Target</span>"));
    assert!(!network.contains("No network events"));

    let attach = between(tools, "function AttachNotice(", "function EmptyEvidence(");
    assert!(attach.contains("translate(locale, 'observer.connectTitle')"));
    assert!(attach.contains("translate(locale, 'observer.connectText')"));
    assert!(!attach.contains("Native observer is not attached"));

    assert!(capture.contains("25-vi-console.png"));
    assert!(capture.contains("26-vi-network.png"));
    assert!(capture.contains("27-vi-live-inspection-unavailable.png"));
}

#[test]
fn primary_chrome_accessible_names_and_panel_eyebrows_use_active_locale() {
    let shell = include_str!("../../src/app/LocalViewShell.tsx");
    let tools = include_str!("../../src/features/FloatingTools.tsx");
    let i18n = include_str!("../../src/i18n.ts");
    let capture = include_str!("../../../../tools/human-first-ui-v2/capture.mjs");

    for key in [
        "aria.localViewControls",
        "aria.localViewTools",
        "aria.inspectorActions",
        "panel.tools",
        "panel.diagnostics",
        "panel.preferences",
        "panel.sessions",
    ] {
        assert!(i18n.contains(&format!("'{key}'")), "missing localized chrome key {key}");
    }

    assert!(shell.contains("translate(preferences.locale, 'aria.localViewControls')"));
    assert!(shell.contains("translate(locale, 'aria.localViewTools')"));
    assert!(tools.contains("aria-label={panelTitle(tool, locale)}"));
    assert!(tools.contains("translate(locale, 'aria.inspectorActions')"));
    assert!(tools.contains("panelEyebrow(tool, locale)"));
    assert!(tools.contains("translate(locale, 'panel.preferences')"));
    assert!(!shell.contains("aria-label=\"LocalView controls\""));
    assert!(!shell.contains("aria-label=\"LocalView tools\""));
    assert!(!tools.contains("aria-label=\"Inspector actions\""));
    assert!(!tools.contains("settings: 'PREFERENCES'"));
    assert!(capture.contains("vi-accessibility:chrome-label"));
    assert!(capture.contains("vi-accessibility:tool-rail-label"));
    assert!(capture.contains("vi-accessibility:inspector-actions-label"));
}

#[test]
fn workspace_disconnect_state_uses_active_locale() {
    let surface = include_str!("../../src/app/WorkspaceSurface.tsx");
    let i18n = include_str!("../../src/i18n.ts");
    let capture = include_str!("../../../../tools/human-first-ui-v2/capture.mjs");

    assert!(i18n.contains("'session.disconnected'"));
    assert!(i18n.contains("'session.reconnectGrace'"));
    assert!(surface.contains("translate(locale, 'session.disconnected')"));
    assert!(surface.contains("translate(locale, 'session.reconnectGrace')"));
    assert!(!surface.contains("Dev server disconnected"));
    assert!(!surface.contains("reconnect grace period"));
    assert!(capture.contains("dashboardDisconnected"));
    assert!(capture.contains("23-vi-disconnected.png"));
    assert!(capture.contains("vi-disconnected:localized-title"));
    assert!(capture.contains("vi-disconnected:localized-guidance"));
}

#[test]
fn render_audit_exercises_settings_interactions_and_live_locale_switching() {
    let capture = include_str!("../../../../tools/human-first-ui-v2/capture.mjs");

    for marker in [
        "settings-target-hidden",
        "settings-rail-hidden",
        "settings-reset-recovered",
        "20-settings-reset-recovered.png",
        "settings-live-locale-switch-en",
        "settings-live-locale-switch-vi",
        "21-settings-live-locale-switch.png",
        "settings-reset-recovered:target-visible",
        "settings-reset-recovered:rail-visible",
    ] {
        assert!(
            capture.contains(marker),
            "render audit is missing Settings interaction marker {marker}"
        );
    }

    assert!(capture.contains("getByLabel('Show target bar').setChecked(false)"));
    assert!(capture.contains("getByLabel('Show tool rail').setChecked(false)"));
    assert!(capture.contains("getByRole('button', { name: 'Reset workspace' }).click()"));
    assert!(capture.contains("locator('.panel-settings select').selectOption('en')"));
    assert!(capture.contains("locator('.panel-settings select').selectOption('vi')"));
    assert!(capture.contains("settingsRecoveredPreferences?.showTargetBar === true"));
    assert!(capture.contains("settingsRecoveredPreferences?.showToolRail === true"));
    assert!(capture.contains("liveLocalePreferences?.locale === 'en'"));
    assert!(capture.contains("liveLocalePreferences?.locale === 'vi'"));
}

#[test]
fn visual_system_uses_muted_moss_instead_of_ai_blue() {
    let styles = include_str!("../../src/styles.css");

    assert!(styles.contains("--lv-accent:#9aa982"));
    assert!(styles.contains("--lv-accent-soft"));
    assert!(!styles.contains("--lv-accent:#8bb6ff"));
}
