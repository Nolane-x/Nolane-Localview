import { useCallback, useEffect, useMemo, useState, type ReactNode } from 'react';
import { api } from '../api';
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
  InspectIcon,
  NetworkIcon,
  PauseIcon,
  PlayIcon,
  ResponsiveIcon,
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

export default function LocalViewShell() {
  const [state, setState] = useState<DashboardState>(fallback);
  const [selected, setSelected] = useState<string>();
  const [activeTool, setActiveTool] = useState<ToolId>();
  const [error, setError] = useState<string>();
  const [live, setLive] = useState<LiveSessionState>(emptyLive);
  const [immersive, setImmersive] = useState(false);

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
      const shortcuts: Record<string, ToolId> = {
        i: 'inspect',
        r: 'responsive',
        c: 'console',
        n: 'network',
        a: 'ai',
      };
      const tool = shortcuts[event.key.toLowerCase()];
      if (tool) toggleTool(tool);
    };
    window.addEventListener('keydown', onKeyDown);
    return () => window.removeEventListener('keydown', onKeyDown);
  }, [toggleTool]);

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
        <TopPill
          state={state}
          current={current}
          selected={selected}
          live={live}
          onSelect={setSelected}
          onSessions={() => toggleTool('sessions')}
          onPause={() => void togglePause()}
          onOpenNative={() => void openNative()}
          onImmersive={() => setImmersive((value) => !value)}
        />
        <FloatingRail activeTool={activeTool} onTool={toggleTool} onCommand={() => toggleTool('command')} />
        {activeTool && (
          <FloatingPanel
            tool={activeTool}
            state={state}
            live={live}
            current={current}
            url={currentUrl}
            onClose={() => setActiveTool(undefined)}
            onSelect={setSelected}
            onOpenNative={() => void openNative()}
            onPause={() => void togglePause()}
          />
        )}
        {error && <RuntimeToast error={error} onRetry={() => void refresh()} />}
      </div>
    </div>
  );
}

function TopPill({ state, current, selected, live, onSelect, onSessions, onPause, onOpenNative, onImmersive }: {
  state: DashboardState;
  current?: Session;
  selected?: string;
  live: LiveSessionState;
  onSelect: (value: string) => void;
  onSessions: () => void;
  onPause: () => void;
  onOpenNative: () => void;
  onImmersive: () => void;
}) {
  const attached = live.observer.length > 0;
  return <header className="top-pill">
    <button className="logo-button" aria-label="Show sessions" onClick={onSessions}><span className="logo-glyph">L</span></button>
    <div className="top-divider"/>
    <div className="target-block">
      <div className="target-row">
        <span className={`health-dot ${state.health.paused ? 'warn' : ''}`}/>
        <select aria-label="Current localhost session" value={current?.id ?? selected ?? ''} onChange={(event) => onSelect(event.target.value)} disabled={!state.sessions.length}>
          {!state.sessions.length && <option value="">Waiting for localhost</option>}
          {state.sessions.map((session) => <option key={session.id} value={session.id}>{session.project.display_name} · :{session.endpoint.port}</option>)}
        </select>
      </div>
      <span className="target-meta">{current ? `${current.classification.framework ?? 'Web'} · ${attached ? 'observer live' : current.classification.hmr_detected ? 'HMR detected' : 'HTTP live'}` : state.health.paused ? 'Detection paused' : 'Auto-discovery active'}</span>
    </div>
    <div className="top-divider"/>
    <div className="top-actions">
      <span className={`live-indicator ${attached ? 'attached' : ''}`} title={attached ? 'Native observer attached' : 'Native observer not attached'}>{attached ? `${live.observer.length} live` : 'observer idle'}</span>
      <IconButton label={state.health.paused ? 'Resume discovery' : 'Pause discovery'} onClick={onPause}>{state.health.paused ? <PlayIcon/> : <PauseIcon/>}</IconButton>
      <IconButton label="Open isolated native preview and observer" onClick={onOpenNative} disabled={!current}><ExternalIcon/></IconButton>
      <IconButton label="Toggle immersive chrome" onClick={onImmersive}><ExpandIcon/></IconButton>
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
