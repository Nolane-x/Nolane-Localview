import { DEFAULT_LOCALE, normalizeLocale, type SupportedLocale } from './i18n';

export interface ChromePoint {
  x: number;
  y: number;
}

export type AnnotationPersistence = 'never' | 'session' | 'project';
export type NotificationPreference = 'off' | 'important' | 'all';
export type AutoOpenPreference = 'never' | 'first_session' | 'frontend_only' | 'always';

export interface LocalViewPreferences {
  version: 2;
  locale: SupportedLocale;
  showTargetBar: boolean;
  showToolRail: boolean;
  rememberChromePositions: boolean;
  targetBarPosition: ChromePoint | null;
  toolRailPosition: ChromePoint | null;
  annotationPersistence: AnnotationPersistence;
  notifications: NotificationPreference;
  autoOpen: AutoOpenPreference;
  reducedMotion: 'system' | 'reduce';
  density: 'comfortable' | 'compact';
  accent: 'muted-moss';
}

const STORAGE_KEY = 'localview.preferences.v2';

export const DEFAULT_PREFERENCES: LocalViewPreferences = {
  version: 2,
  locale: DEFAULT_LOCALE,
  showTargetBar: true,
  showToolRail: true,
  rememberChromePositions: true,
  targetBarPosition: null,
  toolRailPosition: null,
  annotationPersistence: 'session',
  notifications: 'important',
  autoOpen: 'first_session',
  reducedMotion: 'system',
  density: 'comfortable',
  accent: 'muted-moss',
};

function safePoint(value: unknown): ChromePoint | null {
  if (!value || typeof value !== 'object') return null;
  const point = value as Partial<ChromePoint>;
  if (!Number.isFinite(point.x) || !Number.isFinite(point.y)) return null;
  return { x: Number(point.x), y: Number(point.y) };
}

function mergePreferences(value: unknown): LocalViewPreferences {
  if (!value || typeof value !== 'object') return { ...DEFAULT_PREFERENCES };
  const candidate = value as Partial<LocalViewPreferences>;
  return {
    ...DEFAULT_PREFERENCES,
    locale: normalizeLocale(candidate.locale),
    showTargetBar: candidate.showTargetBar !== false,
    showToolRail: candidate.showToolRail !== false,
    rememberChromePositions: candidate.rememberChromePositions !== false,
    targetBarPosition: safePoint(candidate.targetBarPosition),
    toolRailPosition: safePoint(candidate.toolRailPosition),
    annotationPersistence:
      candidate.annotationPersistence === 'never' ||
      candidate.annotationPersistence === 'project'
        ? candidate.annotationPersistence
        : 'session',
    notifications:
      candidate.notifications === 'off' || candidate.notifications === 'all'
        ? candidate.notifications
        : 'important',
    autoOpen:
      candidate.autoOpen === 'never' ||
      candidate.autoOpen === 'frontend_only' ||
      candidate.autoOpen === 'always'
        ? candidate.autoOpen
        : 'first_session',
    reducedMotion: candidate.reducedMotion === 'reduce' ? 'reduce' : 'system',
    density: candidate.density === 'compact' ? 'compact' : 'comfortable',
    accent: 'muted-moss',
  };
}

export function loadPreferences(): LocalViewPreferences {
  try {
    const stored = localStorage.getItem(STORAGE_KEY);
    return stored ? mergePreferences(JSON.parse(stored)) : { ...DEFAULT_PREFERENCES };
  } catch {
    return { ...DEFAULT_PREFERENCES };
  }
}

export function savePreferences(preferences: LocalViewPreferences): void {
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(preferences));
  } catch {
    // User-owned preferences are best-effort; LocalView remains usable without persistence.
  }
}

export function updatePreferences(
  current: LocalViewPreferences,
  patch: Partial<LocalViewPreferences>,
): LocalViewPreferences {
  const next = mergePreferences({ ...current, ...patch, version: 2 });
  savePreferences(next);
  return next;
}

export function resetWorkspace(
  current: LocalViewPreferences,
): LocalViewPreferences {
  const next: LocalViewPreferences = {
    ...current,
    showTargetBar: true,
    showToolRail: true,
    targetBarPosition: null,
    toolRailPosition: null,
  };
  savePreferences(next);
  return next;
}
