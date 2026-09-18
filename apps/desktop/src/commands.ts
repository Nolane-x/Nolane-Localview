export const COMMAND_IDS = {
  workspaceToggleChrome: 'workspace.chrome.toggle',
  workspaceToggleTargetBar: 'workspace.targetBar.toggle',
  workspaceToggleToolRail: 'workspace.toolRail.toggle',
  workspaceResetLayout: 'workspace.resetLayout',
  sessionSwitch: 'session.switch',
  sessionPauseDiscovery: 'session.pauseDiscovery',
  previewOpenNative: 'preview.openNative',
  inspectActivate: 'inspect.activate',
  sourceOpen: 'source.open',
  responsiveOpen: 'responsive.open',
  consoleOpen: 'console.open',
  networkOpen: 'network.open',
  aiAskSelection: 'ai.askSelection',
  aiFixSelection: 'ai.fixSelection',
  advancedOpen: 'advanced.open',
  settingsOpen: 'settings.open',
  languageChange: 'language.change',
} as const;

export type CommandId = (typeof COMMAND_IDS)[keyof typeof COMMAND_IDS];
