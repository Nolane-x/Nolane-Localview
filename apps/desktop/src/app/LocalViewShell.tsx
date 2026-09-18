import { useCallback, useEffect, useMemo, useState, type ReactNode } from 'react';
import { api } from '../api';
import { COMMAND_IDS } from '../commands';
import { applyDocumentLocale, translate } from '../i18n';
import {
  loadPreferences,
  resetWorkspace,
  updatePreferences as persistPreferences,
  type LocalViewPreferences,
} from '../preferences';
import type { DashboardState, LiveSessionState, Session } from '../types';
import { WorkspaceSurface } from './WorkspaceSurface';
import {
  CommandRailButton,
  FloatingPanel,
  RailButton,
  type ToolId,
} from '../features/FloatingTools';
import {
  CommandIcon,
  ConsoleIcon,
  ExpandIcon,
  ExternalIcon,
  HideIcon,
  InspectIcon,
  MoreIcon,
  NetworkIcon,
  PauseIcon,
  PlayIcon,
  ResponsiveIcon,
  SettingsIcon,
  SparkIcon,
} from '../components/icons';

const fallback: DashboardState = {
  health: { version: '0.2.0', status: 'connecting', paused: false, sessions: 0 },
  sessions: [],
  engine: { native: 'Tauri / WRY', tier3: 'Chromium on demand' },
  capabilities: [],
  workspace_surface: {
    compiled: false,
    default_mode: 'iframe',
    reason: 'Waiting for LocalView desktop runtime capability negotiation',
  },
};

const emptyLive: LiveSessionState = { observer: [], action_results: [] };
const TOGGLE_TARGET_BAR_SHORTCUT = 'Ctrl+Shift+T';
const TARGET_BAR_COMMAND = COMMAND_IDS.workspaceToggleTargetBar; // workspace.targetBar.toggle

export default function LocalViewShell() {
  const [state, setState] = useState<DashboardState>(fallback);
  const [selected, setSelected] = useState<string>();
  const [activeTool, setActiveTool] = useState<ToolId>();
  const [error, setError] = useState<string>();
  const [live, setLive] = useState<LiveSessionState>(emptyLive);
  const [immersive, setImmersive] = useState(false);
  const [preferences, setPreferences] = useState<LocalViewPreferences>(() => loadPreferences());

  const patchPreferences = useCallback((patch: Partial<LocalViewPreferences>) => {
    setPreferences((current) => persistPreferences(current, patch));
  }, []);

  const resetWorkspacePreferences = useCallback(() => {
    setPreferences((current) => resetWorkspace(current));
  }, []);

  useEffect(() => {
    applyDocumentLocale(preferences.locale);
  }, [preferences.locale]);

  const refresh = useCallback(async () => {
    try {
      const next = await api.dashboard();
      setState(next);
      setError(undefined);
      setSelected((current) => {
        if (current && next.sessions.some((session) => session.id === current)) return current;
        return next.sessions[0]?.id;
      });
    } catch (cause) {
      setError(String(cause));
    }
  }, []);

  useEffect(() => {
    void refresh();
    const timer = window.setInterval(() => void refresh(), 1400);
    return () => window.clearInterval(timer);
  }, [refresh]);

  const current = useMemo(
    () => state.sessions.find((session) => session.id === selected) ?? state.sessions[0],
    [state.sessions, selected],
  );

  useEffect(() => {
    let cancelled = false;
    if (!current) {
      setLive(emptyLive);
      return;
    }
    const read = async () => {
      try {
        const next = await api.liveSession(current.id);
        if (!cancelled) setLive(next);
      } catch {
        if (!cancelled) setLive(emptyLive);
      }
    };
    void read();
    const timer = window.setInterval(() => void read(), 650);
    return () => {
      cancelled = true;
      window.clearInterval(timer);
    };
  }, [current?.id]);

  const currentUrl = current
    ? `${current.endpoint.scheme}://${current.endpoint.host}:${current.endpoint.port}/`
    : undefined;

  const toggleTool = useCallback((tool: ToolId) => {
    setActiveTool((active) => active === tool ? undefined : tool);
  }, []);

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      const target = event.target as HTMLElement | null;
      if (target?.matches('input,textarea,select,[contenteditable="true"]')) return;
      if (event.key === 'Escape') {
        setActiveTool(undefined);
        return;
      }
      if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === 'k') {
        event.preventDefault();
        toggleTool('command');
        return;
      }
      if ((event.metaKey || event.ctrlKey) && event.key === ',') {
        event.preventDefault();
        toggleTool('settings');
        return;
      }
      if (event.ctrlKey && event.shiftKey && event.key.toLowerCase() === 't') {
        event.preventDefault();
        void TARGET_BAR_COMMAND;
        void TOGGLE_TARGET_BAR_SHORTCUT;
        patchPreferences({ showTargetBar: !preferences.showTargetBar });
        return;
      }
      const shortcuts: Record<string, ToolId> = {
        i: 'inspect',
        r: 'responsive',
        c: 'console',
        n: 'network',
        a: 'ai',
        m: 'advanced',
      };
      const tool = shortcuts[event.key.toLowerCase()];
      if (tool) toggleTool(tool);
    };
    window.addEventListener('keydown', onKeyDown);
    return () => window.removeEventListener('keydown', onKeyDown);
  }, [patchPreferences, preferences.showTargetBar, toggleTool]);

  async function togglePause() {
    state.health.paused ? await api.resume() : await api.pause();
    await refresh();
  }

  async function openNative(session = current) {
    if (!session) return;
    await api.openPreview(
      session.id,
      `${session.endpoint.scheme}://${session.endpoint.host}:${session.endpoint.port}/`,
      session.project.display_name,
    );
  }

  return (
    <div className={`localview ${immersive ? 'is-immersive' : ''}`}>
      <WorkspaceSurface current={current} url={currentUrl} support={state.workspace_surface} />
      <div className="chrome-layer" aria-label="LocalView controls">
        {preferences.showTargetBar && (
          <TopPill
            state={state}
            current={current}
            selected={selected}
            locale={preferences.locale}
            onSelect={setSelected}
            onSessions={() => toggleTool('sessions')}
            onPause={() => void togglePause()}
            onOpenNative={() => void openNative()}
            onImmersive={() => setImmersive((value) => !value)}
            onHideTargetBar={() => patchPreferences({ showTargetBar: false })}
          />
        )}
        {preferences.showToolRail && (
          <FloatingRail activeTool={activeTool} onTool={toggleTool} onCommand={() => toggleTool('command')} />
        )}
        {activeTool && (
          <FloatingPanel
            tool={activeTool}
            state={state}
            live={live}
            current={current}
            url={currentUrl}
            locale={preferences.locale}
            preferences={preferences}
            onClose={() => setActiveTool(undefined)}
            onSelect={setSelected}
            onOpenNative={() => void openNative()}
            onPause={() => void togglePause()}
            onTool={(tool) => setActiveTool(tool)}
            onPreferencesChange={patchPreferences}
            onResetWorkspace={resetWorkspacePreferences}
          />
        )}
        {error && <RuntimeToast error={error} onRetry={() => void refresh()} />}
      </div>
    </div>
  );
}

function TopPill({
  state,
  current,
  selected,
  locale,
  onSelect,
  onSessions,
  onPause,
  onOpenNative,
  onImmersive,
  onHideTargetBar,
}: {
  state: DashboardState;
  current?: Session;
  selected?: string;
  locale: LocalViewPreferences['locale'];
  onSelect: (value: string) => void;
  onSessions: () => void;
  onPause: () => void;
  onOpenNative: () => void;
  onImmersive: () => void;
  onHideTargetBar: () => void;
}) {
  const statusLabel = state.health.paused
    ? translate(locale, 'status.paused')
    : current
      ? translate(locale, 'status.ready')
      : translate(locale, 'status.offline');

  return <header className="top-pill">
    <button className="logo-button" aria-label="Show sessions" onClick={onSessions}><span className="logo-glyph">L</span></button>
    <div className="top-divider"/>
    <div className="target-block">
      <div className="target-row">
        <span className={`health-dot ${state.health.paused ? 'warn' : ''}`}/>
        <select
          aria-label="Current localhost session"
          value={current?.id ?? selected ?? ''}
          onChange={(event) => onSelect(event.target.value)}
          disabled={!state.sessions.length}
        >
          {!state.sessions.length && <option value="">LocalView</option>}
          {state.sessions.map((session) => (
            <option key={session.id} value={session.id}>
              {session.project.display_name} · :{session.endpoint.port}
            </option>
          ))}
        </select>
      </div>
    </div>
    <span className="target-status" aria-label={statusLabel}>{statusLabel}</span>
    <div className="top-divider"/>
    <div className="top-actions">
      <IconButton
        label={state.health.paused ? translate(locale, 'action.resumeDiscovery') : translate(locale, 'action.pauseDiscovery')}
        onClick={onPause}
      >
        {state.health.paused ? <PlayIcon/> : <PauseIcon/>}
      </IconButton>
      <IconButton label={translate(locale, 'action.openPreview')} onClick={onOpenNative} disabled={!current}><ExternalIcon/></IconButton>
      <IconButton label="Immersive" onClick={onImmersive}><ExpandIcon/></IconButton>
      <IconButton label={translate(locale, 'action.hideTargetBar')} onClick={onHideTargetBar}><HideIcon/></IconButton>
    </div>
  </header>;
}

function FloatingRail({ activeTool, onTool, onCommand }: { activeTool?: ToolId; onTool: (tool: ToolId) => void; onCommand: () => void }) {
  return <nav className="floating-rail" aria-label="LocalView tools">
    <RailButton tool="inspect" active={activeTool === 'inspect'} onClick={() => onTool('inspect')}><InspectIcon/></RailButton>
    <RailButton tool="responsive" active={activeTool === 'responsive'} onClick={() => onTool('responsive')}><ResponsiveIcon/></RailButton>
    <RailButton tool="console" active={activeTool === 'console'} onClick={() => onTool('console')}><ConsoleIcon/></RailButton>
    <RailButton tool="network" active={activeTool === 'network'} onClick={() => onTool('network')}><NetworkIcon/></RailButton>
    <div className="rail-divider"/>
    <RailButton tool="ai" active={activeTool === 'ai'} onClick={() => onTool('ai')}><SparkIcon/></RailButton>
    <RailButton tool="settings" active={activeTool === 'settings'} onClick={() => onTool('settings')}><SettingsIcon/></RailButton>
    <RailButton tool="advanced" active={activeTool === 'advanced'} onClick={() => onTool('advanced')}><MoreIcon/></RailButton>
    <CommandRailButton active={activeTool === 'command'} onClick={onCommand}/>
  </nav>;
}

function RuntimeToast({ error, onRetry }: { error: string; onRetry: () => void }) {
  return <div className="runtime-toast" role="status"><span className="health-dot danger"/><div><strong>Runtime unavailable</strong><span>{error}</span></div><button onClick={onRetry}>Retry</button></div>;
}

function IconButton({ label, onClick, disabled, children }: { label: string; onClick: () => void; disabled?: boolean; children: ReactNode }) {
  return <button className="icon-button" aria-label={label} title={label} onClick={onClick} disabled={disabled}>{children}</button>;
}

export function LocalViewCommandGlyph() { return <CommandIcon/>; }
