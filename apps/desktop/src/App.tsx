import { useCallback, useEffect, useMemo, useState, type ReactNode } from 'react';
import { api } from './api';
import type { DashboardState, Session } from './types';

type ToolId = 'inspect' | 'responsive' | 'console' | 'network' | 'ai' | 'sessions' | 'command';

const fallback: DashboardState = {
  health: { version: '0.2.0', status: 'connecting', paused: false, sessions: 0 },
  sessions: [],
  engine: { native: 'Tauri / WRY', tier3: 'Chromium on demand' },
  capabilities: ['Discovery', 'Sessions', 'Semantic diff', 'Layout', 'Visual', 'Responsive', 'A11y', 'MCP'],
};

const toolMeta: Record<Exclude<ToolId, 'sessions' | 'command'>, { label: string; shortcut: string }> = {
  inspect: { label: 'Inspect / X-Ray', shortcut: 'I' },
  responsive: { label: 'Responsive', shortcut: 'R' },
  console: { label: 'Console', shortcut: 'C' },
  network: { label: 'Network', shortcut: 'N' },
  ai: { label: 'AI Critic', shortcut: 'A' },
};

export default function App() {
  const [state, setState] = useState<DashboardState>(fallback);
  const [selected, setSelected] = useState<string>();
  const [activeTool, setActiveTool] = useState<ToolId>();
  const [error, setError] = useState<string>();
  const [immersive, setImmersive] = useState(false);

  const refresh = useCallback(async () => {
    try {
      const next = await api.dashboard();
      setState(next);
      setError(undefined);
      setSelected((current) => current ?? next.sessions[0]?.id);
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
      const map: Record<string, ToolId> = {
        i: 'inspect',
        r: 'responsive',
        c: 'console',
        n: 'network',
        a: 'ai',
      };
      const tool = map[event.key.toLowerCase()];
      if (tool) toggleTool(tool);
    };
    window.addEventListener('keydown', onKeyDown);
    return () => window.removeEventListener('keydown', onKeyDown);
  }, [toggleTool]);

  async function togglePause() {
    state.health.paused ? await api.resume() : await api.pause();
    await refresh();
  }

  async function openNative(session: Session) {
    await api.openPreview(
      session.id,
      `${session.endpoint.scheme}://${session.endpoint.host}:${session.endpoint.port}/`,
      session.project.display_name,
    );
  }

  return (
    <div className={`localview ${immersive ? 'is-immersive' : ''}`}>
      <Workspace current={current} url={currentUrl} />

      <div className="chrome-layer" aria-label="LocalView controls">
        <TopPill
          state={state}
          current={current}
          selected={selected}
          onSelect={setSelected}
          onSessions={() => toggleTool('sessions')}
          onPause={() => void togglePause()}
          onOpenNative={() => current && void openNative(current)}
          onImmersive={() => setImmersive((value) => !value)}
        />

        <FloatingRail
          activeTool={activeTool}
          onTool={toggleTool}
          onCommand={() => toggleTool('command')}
        />

        {activeTool && (
          <FloatingPanel
            tool={activeTool}
            state={state}
            current={current}
            url={currentUrl}
            onClose={() => setActiveTool(undefined)}
            onSelect={setSelected}
            onOpenNative={() => current && void openNative(current)}
            onPause={() => void togglePause()}
          />
        )}

        {error && (
          <div className="runtime-toast" role="status">
            <span className="health-dot danger" />
            <div>
              <strong>Runtime unavailable</strong>
              <span>{error}</span>
            </div>
            <button onClick={() => void refresh()}>Retry</button>
          </div>
        )}
      </div>
    </div>
  );
}

function Workspace({ current, url }: { current?: Session; url?: string }) {
  if (!current || !url) {
    return (
      <main className="workspace workspace-empty">
        <div className="empty-orbit" aria-hidden="true">
          <i /><i /><i />
          <span />
        </div>
        <div className="empty-copy">
          <span className="micro-label">LOCALVIEW RUNTIME</span>
          <h1>Your localhost becomes the workspace.</h1>
          <p>Run a frontend dev server. LocalView discovers it automatically and keeps the interface out of the way until you ask for a tool.</p>
          <div className="empty-command"><kbd>⌘</kbd><kbd>K</kbd><span>Open command palette</span></div>
        </div>
      </main>
    );
  }

  return (
    <main className="workspace">
      <iframe
        key={url}
        className="app-frame"
        src={url}
        title={`${current.project.display_name} local preview`}
        referrerPolicy="no-referrer"
      />
      {current.status === 'disconnected' && (
        <div className="disconnect-shade">
          <div><span className="health-dot danger" /><strong>Dev server disconnected</strong><p>LocalView is preserving this session during the reconnect grace period.</p></div>
        </div>
      )}
    </main>
  );
}

function TopPill({
  state,
  current,
  selected,
  onSelect,
  onSessions,
  onPause,
  onOpenNative,
  onImmersive,
}: {
  state: DashboardState;
  current?: Session;
  selected?: string;
  onSelect: (value: string) => void;
  onSessions: () => void;
  onPause: () => void;
  onOpenNative: () => void;
  onImmersive: () => void;
}) {
  return (
    <header className="top-pill">
      <button className="logo-button" aria-label="Show sessions" onClick={onSessions}>
        <span className="logo-glyph">L</span>
      </button>
      <div className="top-divider" />
      <div className="target-block">
        <div className="target-row">
          <span className={`health-dot ${state.health.paused ? 'warn' : ''}`} />
          <select
            aria-label="Current localhost session"
            value={current?.id ?? selected ?? ''}
            onChange={(event) => onSelect(event.target.value)}
            disabled={state.sessions.length === 0}
          >
            {state.sessions.length === 0 && <option value="">Waiting for localhost</option>}
            {state.sessions.map((session) => (
              <option key={session.id} value={session.id}>
                {session.project.display_name} · :{session.endpoint.port}
              </option>
            ))}
          </select>
        </div>
        <span className="target-meta">
          {current
            ? `${current.classification.framework ?? 'Web'} · ${current.classification.hmr_detected ? 'HMR live' : 'HTTP live'}`
            : state.health.paused ? 'Detection paused' : 'Auto-discovery active'}
        </span>
      </div>
      <div className="top-divider" />
      <div className="top-actions">
        <IconButton label={state.health.paused ? 'Resume discovery' : 'Pause discovery'} onClick={onPause}>
          {state.health.paused ? <PlayIcon /> : <PauseIcon />}
        </IconButton>
        <IconButton label="Open isolated native preview" onClick={onOpenNative} disabled={!current}>
          <ExternalIcon />
        </IconButton>
        <IconButton label="Toggle immersive chrome" onClick={onImmersive}>
          <ExpandIcon />
        </IconButton>
      </div>
    </header>
  );
}

function FloatingRail({
  activeTool,
  onTool,
  onCommand,
}: {
  activeTool?: ToolId;
  onTool: (tool: ToolId) => void;
  onCommand: () => void;
}) {
  return (
    <nav className="floating-rail" aria-label="LocalView tools">
      <RailButton tool="inspect" active={activeTool === 'inspect'} onClick={() => onTool('inspect')}><InspectIcon /></RailButton>
      <RailButton tool="responsive" active={activeTool === 'responsive'} onClick={() => onTool('responsive')}><ResponsiveIcon /></RailButton>
      <RailButton tool="console" active={activeTool === 'console'} onClick={() => onTool('console')}><ConsoleIcon /></RailButton>
      <RailButton tool="network" active={activeTool === 'network'} onClick={() => onTool('network')}><NetworkIcon /></RailButton>
      <div className="rail-divider" />
      <RailButton tool="ai" active={activeTool === 'ai'} onClick={() => onTool('ai')}><SparkIcon /></RailButton>
      <button className={`rail-button command ${activeTool === 'command' ? 'active' : ''}`} onClick={onCommand} aria-label="Command palette">
        <CommandIcon /><span className="rail-tooltip">Command palette <kbd>⌘K</kbd></span>
      </button>
    </nav>
  );
}

function RailButton({
  tool,
  active,
  onClick,
  children,
}: {
  tool: Exclude<ToolId, 'sessions' | 'command'>;
  active: boolean;
  onClick: () => void;
  children: ReactNode;
}) {
  const meta = toolMeta[tool];
  return (
    <button className={`rail-button ${active ? 'active' : ''}`} onClick={onClick} aria-pressed={active} aria-label={meta.label}>
      {children}
      <span className="rail-tooltip">{meta.label}<kbd>{meta.shortcut}</kbd></span>
    </button>
  );
}

function FloatingPanel({
  tool,
  state,
  current,
  url,
  onClose,
  onSelect,
  onOpenNative,
  onPause,
}: {
  tool: ToolId;
  state: DashboardState;
  current?: Session;
  url?: string;
  onClose: () => void;
  onSelect: (id: string) => void;
  onOpenNative: () => void;
  onPause: () => void;
}) {
  const bottomSheet = tool === 'console' || tool === 'network';
  const compact = tool === 'command';
  return (
    <section className={`floating-panel panel-${tool} ${bottomSheet ? 'bottom-sheet' : ''} ${compact ? 'command-panel' : ''}`} aria-label={`${tool} panel`}>
      <PanelHeader
        title={panelTitle(tool)}
        eyebrow={panelEyebrow(tool)}
        onClose={onClose}
      />
      <div className="panel-body">
        {tool === 'inspect' && <Inspector current={current} />}
        {tool === 'responsive' && <ResponsivePanel current={current} />}
        {tool === 'console' && <ConsolePanel />}
        {tool === 'network' && <NetworkPanel current={current} />}
        {tool === 'ai' && <AiPanel current={current} />}
        {tool === 'sessions' && <SessionsPanel state={state} current={current} onSelect={onSelect} />}
        {tool === 'command' && (
          <CommandPanel
            state={state}
            current={current}
            url={url}
            onOpenNative={onOpenNative}
            onPause={onPause}
          />
        )}
      </div>
    </section>
  );
}

function PanelHeader({ title, eyebrow, onClose }: { title: string; eyebrow: string; onClose: () => void }) {
  return (
    <div className="panel-header">
      <div><span>{eyebrow}</span><strong>{title}</strong></div>
      <button className="close-button" aria-label={`Close ${title}`} onClick={onClose}><CloseIcon /></button>
    </div>
  );
}

function Inspector({ current }: { current?: Session }) {
  if (!current) return <PanelEmpty title="No active target" text="Start a localhost frontend to inspect its runtime identity and semantic surface." />;
  return (
    <div className="inspector-stack">
      <div className="inspector-hero">
        <div className="selection-cross"><InspectIcon /></div>
        <div><span>Current target</span><strong>{current.project.display_name}</strong><p>{current.classification.framework ?? 'Generic web runtime'}</p></div>
      </div>
      <InfoGrid
        rows={[
          ['Status', current.status],
          ['Confidence', `${Math.round(current.classification.confidence * 100)}%`],
          ['HMR', current.classification.hmr_detected ? 'Detected' : 'Not detected'],
          ['Port', String(current.endpoint.port)],
        ]}
      />
      <div className="panel-section">
        <SectionLabel title="Project identity" aside="port-independent" />
        <code className="path-block">{current.project.git_root ?? current.project.cwd ?? 'Process-derived project identity'}</code>
      </div>
      <div className="panel-section">
        <SectionLabel title="X-Ray pipeline" aside="progressive disclosure" />
        <div className="pipeline-list">
          <PipelineStep n="01" title="Semantic refs" state="ready" />
          <PipelineStep n="02" title="Geometry + layout evidence" state="ready" />
          <PipelineStep n="03" title="Source-map ranking" state="ready" />
          <PipelineStep n="04" title="Live observer drain" state="wiring" />
        </div>
      </div>
    </div>
  );
}

function ResponsivePanel({ current }: { current?: Session }) {
  const presets = [
    ['Mobile S', '320', '568'],
    ['Mobile', '390', '844'],
    ['Tablet', '768', '1024'],
    ['Desktop', '1440', '900'],
  ];
  return (
    <div>
      <div className="responsive-summary">
        <span>ADAPTIVE SWEEP</span>
        <strong>{current ? current.project.display_name : 'No target'}</strong>
        <p>Presets are only anchors. LocalView’s Rust responsive engine also searches for meaningful breakpoints between them.</p>
      </div>
      <div className="viewport-list">
        {presets.map(([name, width, height]) => (
          <button key={name} disabled={!current}>
            <span className="viewport-icon" />
            <div><strong>{name}</strong><span>{width} × {height}</span></div>
            <kbd>{width}</kbd>
          </button>
        ))}
      </div>
      <div className="panel-note">Viewport contact sheets and binary breakpoint discovery stay on-demand so the normal workspace remains untouched.</div>
    </div>
  );
}

function ConsolePanel() {
  return (
    <div className="stream-panel">
      <div className="stream-toolbar">
        <span className="filter-chip active">All</span><span className="filter-chip">Errors</span><span className="filter-chip">Warnings</span>
        <span className="stream-status"><i /> observer ready</span>
      </div>
      <div className="stream-empty">
        <ConsoleIcon />
        <strong>No console events in this UI surface yet</strong>
        <p>The injected observer already captures warn/error/unhandled rejection events. Secure native draining into the daemon is the next transport step.</p>
      </div>
    </div>
  );
}

function NetworkPanel({ current }: { current?: Session }) {
  return (
    <div className="stream-panel">
      <div className="network-summary">
        <div><span>Target</span><strong>{current ? `:${current.endpoint.port}` : '—'}</strong></div>
        <div><span>Analyzer</span><strong>Failed · Slow · Duplicate · CORS</strong></div>
        <div><span>Policy</span><strong>Local evidence first</strong></div>
      </div>
      <div className="stream-empty compact-empty">
        <NetworkIcon />
        <strong>Network evidence stays quiet until it matters.</strong>
        <p>LocalView’s analyzer is implemented; the floating sheet is intentionally empty rather than fabricating traffic before the live bridge is connected.</p>
      </div>
    </div>
  );
}

function AiPanel({ current }: { current?: Session }) {
  return (
    <div className="ai-panel-content">
      <div className="ai-mark"><SparkIcon /></div>
      <span className="micro-label">POINT · ASK · DIAGNOSE</span>
      <h2>Ask about what you can see.</h2>
      <p>{current ? `Targeting ${current.project.display_name}.` : 'Start a target to enable grounded visual questions.'} AI actions will consume semantic/layout/telemetry evidence before escalating to heavier browser capture.</p>
      <div className="prompt-box">
        <span>Ask LocalView AI…</span>
        <kbd>↵</kbd>
      </div>
      <div className="suggestion-grid">
        <button disabled={!current}>Why is this misaligned?</button>
        <button disabled={!current}>Find the source component</button>
        <button disabled={!current}>Check responsive breakpoints</button>
        <button disabled={!current}>Explain the last visual diff</button>
      </div>
    </div>
  );
}

function SessionsPanel({
  state,
  current,
  onSelect,
}: {
  state: DashboardState;
  current?: Session;
  onSelect: (id: string) => void;
}) {
  return (
    <div className="sessions-panel">
      <div className="session-overview">
        <strong>{state.sessions.length}</strong><span>detected localhost session{state.sessions.length === 1 ? '' : 's'}</span>
      </div>
      <div className="session-cards">
        {state.sessions.map((session) => (
          <button key={session.id} className={current?.id === session.id ? 'selected' : ''} onClick={() => onSelect(session.id)}>
            <span className={`health-dot ${session.status === 'disconnected' ? 'danger' : session.status === 'hidden' ? 'warn' : ''}`} />
            <div><strong>{session.project.display_name}</strong><span>{session.classification.framework ?? 'Web'} · :{session.endpoint.port}</span></div>
            <span className="session-state">{session.status}</span>
          </button>
        ))}
        {state.sessions.length === 0 && <PanelEmpty title="Nothing detected" text="LocalView watches loopback listeners and classifies frontend candidates automatically." />}
      </div>
    </div>
  );
}

function CommandPanel({
  state,
  current,
  url,
  onOpenNative,
  onPause,
}: {
  state: DashboardState;
  current?: Session;
  url?: string;
  onOpenNative: () => void;
  onPause: () => void;
}) {
  const commands = [
    { icon: <InspectIcon />, title: 'Inspect element', detail: 'Enter X-Ray selection mode', keys: 'I' },
    { icon: <ResponsiveIcon />, title: 'Responsive audit', detail: 'Adaptive viewport sweep', keys: 'R' },
    { icon: <SparkIcon />, title: 'Ask AI about current view', detail: 'Grounded visual diagnosis', keys: 'A' },
    { icon: <ExternalIcon />, title: 'Open native preview', detail: url ?? 'No active URL', keys: '↵', action: onOpenNative, disabled: !current },
    { icon: state.health.paused ? <PlayIcon /> : <PauseIcon />, title: state.health.paused ? 'Resume discovery' : 'Pause discovery', detail: 'Runtime listener scanning', keys: 'P', action: onPause },
  ];
  return (
    <div className="command-content">
      <div className="command-search"><SearchIcon /><input autoFocus placeholder="Type a command…" aria-label="Search commands" /><kbd>ESC</kbd></div>
      <div className="command-list">
        {commands.map((command) => (
          <button key={command.title} onClick={command.action} disabled={command.disabled}>
            <span className="command-icon">{command.icon}</span>
            <div><strong>{command.title}</strong><span>{command.detail}</span></div>
            <kbd>{command.keys}</kbd>
          </button>
        ))}
      </div>
      <div className="command-footer"><span>LocalView v{state.health.version}</span><span>Diff-first · localhost-only control</span></div>
    </div>
  );
}

function InfoGrid({ rows }: { rows: [string, string][] }) {
  return <div className="info-grid">{rows.map(([label, value]) => <div key={label}><span>{label}</span><strong>{value}</strong></div>)}</div>;
}

function PipelineStep({ n, title, state }: { n: string; title: string; state: 'ready' | 'wiring' }) {
  return <div className="pipeline-step"><span>{n}</span><strong>{title}</strong><em className={state}>{state}</em></div>;
}

function SectionLabel({ title, aside }: { title: string; aside: string }) {
  return <div className="section-label"><strong>{title}</strong><span>{aside}</span></div>;
}

function PanelEmpty({ title, text }: { title: string; text: string }) {
  return <div className="panel-empty"><span className="empty-pulse" /><strong>{title}</strong><p>{text}</p></div>;
}

function IconButton({ label, onClick, disabled, children }: { label: string; onClick: () => void; disabled?: boolean; children: ReactNode }) {
  return <button className="icon-button" aria-label={label} title={label} onClick={onClick} disabled={disabled}>{children}</button>;
}

function panelTitle(tool: ToolId) {
  return {
    inspect: 'Inspector',
    responsive: 'Responsive Lab',
    console: 'Console',
    network: 'Network',
    ai: 'AI Critic',
    sessions: 'Local sessions',
    command: 'Command palette',
  }[tool];
}

function panelEyebrow(tool: ToolId) {
  return {
    inspect: 'X-RAY · STRUCTURE',
    responsive: 'VIEWPORT · LAYOUT',
    console: 'BEHAVIOR · TELEMETRY',
    network: 'REQUEST · EVIDENCE',
    ai: 'VISION · SEMANTICS',
    sessions: 'AUTO-DISCOVERY',
    command: 'LOCALVIEW',
  }[tool];
}

const iconProps = { viewBox: '0 0 24 24', fill: 'none', stroke: 'currentColor', strokeWidth: 1.7, strokeLinecap: 'round' as const, strokeLinejoin: 'round' as const };
function InspectIcon() { return <svg {...iconProps}><circle cx="12" cy="12" r="3"/><path d="M3 8V4h4M17 4h4v4M21 16v4h-4M7 20H3v-4"/></svg>; }
function ResponsiveIcon() { return <svg {...iconProps}><rect x="3" y="5" width="13" height="14" rx="2"/><rect x="18" y="8" width="3" height="9" rx="1"/><path d="M8 16h3"/></svg>; }
function ConsoleIcon() { return <svg {...iconProps}><rect x="3" y="4" width="18" height="16" rx="2"/><path d="m7 9 3 3-3 3M13 15h4"/></svg>; }
function NetworkIcon() { return <svg {...iconProps}><circle cx="5" cy="12" r="2"/><circle cx="19" cy="6" r="2"/><circle cx="19" cy="18" r="2"/><path d="m7 11 10-4M7 13l10 4"/></svg>; }
function SparkIcon() { return <svg {...iconProps}><path d="m12 3 1.2 4.3L17 9l-3.8 1.7L12 15l-1.2-4.3L7 9l3.8-1.7L12 3ZM18.5 14l.7 2.3 2.3.7-2.3.7-.7 2.3-.7-2.3-2.3-.7 2.3-.7.7-2.3ZM5 14l.6 1.9 1.9.6-1.9.6L5 19l-.6-1.9-1.9-.6 1.9-.6L5 14Z"/></svg>; }
function CommandIcon() { return <svg {...iconProps}><path d="M9 6h6M9 12h6M9 18h6M5 6h.01M5 12h.01M5 18h.01"/></svg>; }
function SearchIcon() { return <svg {...iconProps}><circle cx="11" cy="11" r="6"/><path d="m16 16 4 4"/></svg>; }
function CloseIcon() { return <svg {...iconProps}><path d="m7 7 10 10M17 7 7 17"/></svg>; }
function PauseIcon() { return <svg {...iconProps}><path d="M9 6v12M15 6v12"/></svg>; }
function PlayIcon() { return <svg {...iconProps}><path d="m9 6 9 6-9 6V6Z"/></svg>; }
function ExternalIcon() { return <svg {...iconProps}><path d="M14 4h6v6M20 4l-9 9"/><path d="M18 13v5a2 2 0 0 1-2 2H6a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2h5"/></svg>; }
function ExpandIcon() { return <svg {...iconProps}><path d="M8 3H3v5M16 3h5v5M21 16v5h-5M3 16v5h5"/></svg>; }
