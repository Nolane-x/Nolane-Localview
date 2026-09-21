export type ShortcutPlatform = 'mac' | 'windows' | 'linux' | 'unknown';

export interface ShortcutSpec {
  key: string;
  commandOrControl?: boolean;
}

export function detectShortcutPlatform(): ShortcutPlatform {
  if (typeof navigator === 'undefined') return 'unknown';
  const platform = navigator.platform?.toLowerCase() ?? '';
  if (platform.includes('mac') || platform.includes('iphone') || platform.includes('ipad') || platform.includes('ipod')) {
    return 'mac';
  }
  if (platform.includes('win')) return 'windows';
  if (platform.includes('linux') || platform.includes('x11')) return 'linux';
  return 'unknown';
}

export function formatShortcut(
  shortcut: ShortcutSpec,
  platform: ShortcutPlatform = detectShortcutPlatform(),
): string {
  if (!shortcut.commandOrControl) return shortcut.key;
  if (platform === 'mac') return `⌘${shortcut.key}`;
  if (platform === 'windows' || platform === 'linux') return `Ctrl+${shortcut.key}`;
  return `Ctrl/⌘+${shortcut.key}`;
}
