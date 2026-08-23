import type { ReactNode } from 'react';
import type { DashboardState, LiveSessionState, ObserverEvent, Session } from '../types';
import {
  ActivityIcon,
  CloseIcon,
  CommandIcon,
  ConsoleIcon,
  ExternalIcon,
  InspectIcon,
  NetworkIcon,
  PauseIcon,
  PlayIcon,
  ResponsiveIcon,
  SearchIcon,
  SparkIcon,
  WarningIcon,
} from '../components/icons';

export type ToolId = 'inspect' | 'responsive' | 'console' | 'network' | 'ai' | 'sessions' | 'command';

export const toolMeta: Record<Exclude<ToolId, 'sessions' | 'command'>, { label: string; shortcut: string }> = {
  inspect: { label: 'Inspect / X-Ray', shortcut: 'I' },
  responsive: { label: 'Responsive', shortcut: 'R' },
  console: { label: 'Console', shortcut: 'C' },
  network: { label: 'Network', shortcut: 'N' },
  ai: { label: 'AI Critic', shortcut: 'A' },
};

interface FloatingPanelProps {
  tool: ToolId;
  state: DashboardState;
  live: LiveSessionState;
  current?: Session;
  url?: string;
  onClose: () => void;
  onSelect: (id: string) => void;
  onOpenNative: () => void;
  onPause: () => void;
}

export function FloatingPanel({ tool, state, live, current, url, onClose, onSelect, onOpenNative, onPause }: FloatingPanelProps) {
  const bottomSheet = tool === 'console' || tool === 'network';
  const compact = tool === 'command';
  return (
    <section className={`floating-panel panel-${tool} ${bottomSheet ? 'bottom-sheet' : ''} ${compact ? 'command-panel' : ''}`} aria-label={`${tool} panel`}>
      <PanelHeader title={panelTitle(tool)} eyebrow={panelEyebrow(tool)} onClose={onClose} />
      <div className="panel-body">
        {tool === 'inspect' && <Inspector current={current} live={live} onOpenNative={onOpenNative} />}
        {tool === 'responsive' && <ResponsivePanel current={current} />}
        {tool === 'console' && <ConsolePanel live={live} onOpenNative={onOpenNative} />}
        {tool === 'network' && <NetworkPanel current={current} live={live} onOpenNative={onOpenNative} />}
        {tool === 'ai' && <AiPanel current={current} live={live} />}
        {tool === 'sessions' && <SessionsPanel state={state} current={current} onSelect={onSelect} />}
        {tool === 'command' && <CommandPanel state={state} current={current} live={live} url={url} onOpenNative={onOpenNative} onPause={onPause} />}
      </div>
    </section>
  );
}

function PanelHeader({ title, eyebrow, onClose }: { title: string; eyebrow: string; onClose: () => void }) {
  return <div className="panel-header"><div><span>{eyebrow}</span><strong>{title}</strong></div><button className="close-button" aria-label={`Close ${title}`} onClick={onClose}><CloseIcon /></button></div>;
}

function Inspector({ current, live, onOpenNative }: { current?: Session; live: LiveSessionState; onOpenNative: () => void }) {
  if (!current) return <PanelEmpty title="No active target" text="Start a localhost frontend to inspect its runtime identity and semantic surface." />;
  const snapshot = [...live.observer].reverse().find((event) => event.kind === 'semantic_snapshot');
  const focused = [...live.observer].reverse().find((event) => event.kind === 'focus');
  const latest = live.observer.at(-1);
  return (
    <div className="inspector-stack">
      <div className="inspector-hero">
        <div className="selection-cross"><InspectIcon /></div>
        <div><span>Current target</span><strong>{current.project.display_name}</strong><p>{current.classification.framework ?? 'Generic web runtime'}</p></div>
      </div>
      <InfoGrid rows={[
        ['Status', current.status],
        ['Observer', live.observer.length ? `${live.observer.length} events` : 'not attached'],
        ['Latest', latest?.kind ?? '—'],
        ['Focused ref', focused?.reference ?? '—'],
      ]} />
      {!live.observer.length && <AttachNotice onOpenNative={onOpenNative} />}
      {snapshot && <EvidenceCard event={snapshot} />}
      <div className="panel-section">
        <SectionLabel title="Project identity" aside="port-independent" />
        <code className="path-block">{current.project.git_root ?? current.project.cwd ?? 'Process-derived project identity'}</code>
      </div>
      <div className="panel-section">
        <SectionLabel title="X-Ray pipeline" aside="live evidence" />
        <div className="pipeline-list">
          <PipelineStep n="01" title="Semantic refs" state="ready" />
          <PipelineStep n="02" title="Geometry + layout evidence" state="ready" />
          <PipelineStep n="03" title="Source-map ranking" state="ready" />
          <PipelineStep n="04" title="Secure native observer drain" state={live.observer.length ? 'ready' : 'idle'} />
        </div>
      </div>
    </div>
  );
}

function ResponsivePanel({ current }: { current?: Session }) {
  const presets = [['Mobile S', '320', '568'], ['Mobile', '390', '844'], ['Tablet', '768', '1024'], ['Desktop', '1440', '900']];
  return <div>
    <div className="responsive-summary"><span>ADAPTIVE SWEEP</span><strong>{current ? current.project.display_name : 'No target'}</strong><p>Preset anchors stay lightweight; the Rust responsive engine can binary-search breakpoints only after layout evidence indicates a failure interval.</p></div>
    <div className="viewport-list">{presets.map(([name, width, height]) => <button key={name} disabled={!current}><span className="viewport-icon"/><div><strong>{name}</strong><span>{width} × {height}</span></div><kbd>{width}</kbd></button>)}</div>
    <div className="panel-note">Responsive tooling is on-demand so the normal workspace never shrinks.</div>
  </div>;
}

function ConsolePanel({ live, onOpenNative }: { live: LiveSessionState; onOpenNative: () => void }) {
  const events = live.observer.filter((event) => event.kind === 'console' || event.kind === 'runtime_error').slice(-80);
  return <div className="stream-panel">
    <div className="stream-toolbar"><span className="filter-chip active">Live</span><span className="stream-status"><i className={events.length ? '' : 'muted'} /> {events.length} event{events.length === 1 ? '' : 's'}</span></div>
    {events.length ? <div className="evidence-stream">{events.map((event) => <ConsoleRow key={`${event.seq}-${event.captured_at}`} event={event} />)}</div> : <EmptyEvidence icon={<ConsoleIcon />} title="No console evidence yet" text="Open the native preview to attach the secure observer. LocalView stays silent instead of inventing console traffic." action="Attach observer" onAction={onOpenNative} />}
  </div>;
}

function NetworkPanel({ current, live, onOpenNative }: { current?: Session; live: LiveSessionState; onOpenNative: () => void }) {
  const events = live.observer.filter((event) => event.kind === 'network').slice(-100);
  const failures = events.filter((event) => Number(event.payload.status ?? 0) >= 400 || event.payload.ok === false).length;
  return <div className="stream-panel">
    <div className="network-summary">
      <div><span>Target</span><strong>{current ? `:${current.endpoint.port}` : '—'}</strong></div>
      <div><span>Requests</span><strong>{events.length}</strong></div>
      <div><span>Failures</span><strong>{failures}</strong></div>
    </div>
    {events.length ? <div className="evidence-stream network-stream">{events.map((event) => <NetworkRow key={`${event.seq}-${event.captured_at}`} event={event} />)}</div> : <EmptyEvidence icon={<NetworkIcon />} title="No network evidence yet" text="Fetch/XHR metadata is captured without bodies. Open the native preview to attach the live bridge." action="Attach observer" onAction={onOpenNative} />}
  </div>;
}

function AiPanel({ current, live }: { current?: Session; live: LiveSessionState }) {
  const evidenceCount = live.observer.length + live.action_results.length;
  return <div className="ai-panel-content">
    <div className="ai-mark"><SparkIcon /></div><span className="micro-label">POINT · ASK · DIAGNOSE</span><h2>Evidence before opinion.</h2>
    <p>{current ? `Targeting ${current.project.display_name}.` : 'Start a target to enable grounded visual questions.'} {evidenceCount ? `${evidenceCount} live evidence item(s) are available for grounding.` : 'No live evidence is attached yet.'}</p>
    <div className="prompt-box"><span>AI judgement provider is intentionally not hard-wired</span><kbd>local</kbd></div>
    <div className="suggestion-grid"><button disabled>Why is this misaligned?</button><button disabled>Find the source component</button><button disabled>Verify this fix</button><button disabled>Explain the visual diff</button></div>
    <div className="panel-note">Subjective AI judgement remains model-agnostic and must cite evidence IDs before LocalView treats it as actionable.</div>
  </div>;
}

function SessionsPanel({ state, current, onSelect }: { state: DashboardState; current?: Session; onSelect: (id: string) => void }) {
  return <div className="sessions-panel"><div className="session-overview"><strong>{state.sessions.length}</strong><span>detected localhost session{state.sessions.length === 1 ? '' : 's'}</span></div><div className="session-cards">
    {state.sessions.map((session) => <button key={session.id} className={current?.id === session.id ? 'selected' : ''} onClick={() => onSelect(session.id)}><span className={`health-dot ${session.status === 'disconnected' ? 'danger' : session.status === 'hidden' ? 'warn' : ''}`} /><div><strong>{session.project.display_name}</strong><span>{session.classification.framework ?? 'Web'} · :{session.endpoint.port}</span></div><span className="session-state">{session.status}</span></button>)}
    {!state.sessions.length && <PanelEmpty title="Nothing detected" text="LocalView watches loopback listeners and classifies frontend candidates automatically." />}
  </div></div>;
}

function CommandPanel({ state, current, live, url, onOpenNative, onPause }: { state: DashboardState; current?: Session; live: LiveSessionState; url?: string; onOpenNative: () => void; onPause: () => void }) {
  const commands = [
    { icon: <InspectIcon />, title: 'Inspect evidence', detail: `${live.observer.length} live observer events`, keys: 'I', disabled: !current },
    { icon: <ResponsiveIcon />, title: 'Responsive lab', detail: 'Adaptive viewport sweep', keys: 'R', disabled: !current },
    { icon: <ActivityIcon />, title: 'Attach native observer', detail: url ?? 'No active URL', keys: '↵', action: onOpenNative, disabled: !current },
    { icon: state.health.paused ? <PlayIcon /> : <PauseIcon />, title: state.health.paused ? 'Resume discovery' : 'Pause discovery', detail: 'Runtime listener scanning', keys: 'P', action: onPause },
  ];
  return <div className="command-content"><div className="command-search"><SearchIcon /><input autoFocus placeholder="Type a command…" aria-label="Search commands" /><kbd>ESC</kbd></div><div className="command-list">{commands.map((command) => <button key={command.title} onClick={command.action} disabled={command.disabled}><span className="command-icon">{command.icon}</span><div><strong>{command.title}</strong><span>{command.detail}</span></div><kbd>{command.keys}</kbd></button>)}</div><div className="command-footer"><span>LocalView v{state.health.version}</span><span>diff-first · evidence-first · localhost-only</span></div></div>;
}

function ConsoleRow({ event }: { event: ObserverEvent }) {
  const level = String(event.payload.level ?? (event.kind === 'runtime_error' ? 'error' : 'log'));
  const message = String(event.payload.message ?? event.kind);
  return <div className={`evidence-row console-${level}`}><span className="evidence-time">{time(event.captured_at)}</span><span className="evidence-kind">{level}</span><code>{message}</code></div>;
}

function NetworkRow({ event }: { event: ObserverEvent }) {
  const method = String(event.payload.method ?? 'GET');
  const status = event.payload.status == null ? 'ERR' : String(event.payload.status);
  const duration = Number(event.payload.duration ?? 0);
  const url = String(event.payload.url ?? 'unknown request');
  return <div className="evidence-row network-row"><span className="evidence-kind">{method}</span><strong className={Number(status) >= 400 || status === 'ERR' ? 'danger-text' : ''}>{status}</strong><code title={url}>{url}</code><span>{duration.toFixed(1)} ms</span></div>;
}

function EvidenceCard({ event }: { event: ObserverEvent }) {
  return <div className="live-card"><div><ActivityIcon /><strong>{event.kind.replaceAll('_', ' ')}</strong><span>{time(event.captured_at)}</span></div><pre>{JSON.stringify(event.payload, null, 2)}</pre></div>;
}

function AttachNotice({ onOpenNative }: { onOpenNative: () => void }) {
  return <button className="attach-notice" onClick={onOpenNative}><WarningIcon /><div><strong>Native observer is not attached</strong><span>Open the isolated preview to stream semantic, console, network and interaction evidence.</span></div><ExternalIcon /></button>;
}

function EmptyEvidence({ icon, title, text, action, onAction }: { icon: ReactNode; title: string; text: string; action: string; onAction: () => void }) {
  return <div className="stream-empty compact-empty">{icon}<strong>{title}</strong><p>{text}</p><button className="quiet-action" onClick={onAction}>{action}</button></div>;
}

function InfoGrid({ rows }: { rows: [string, string][] }) { return <div className="info-grid">{rows.map(([label, value]) => <div key={label}><span>{label}</span><strong>{value}</strong></div>)}</div>; }
function PipelineStep({ n, title, state }: { n: string; title: string; state: 'ready' | 'idle' }) { return <div className="pipeline-step"><span>{n}</span><strong>{title}</strong><em className={state}>{state}</em></div>; }
function SectionLabel({ title, aside }: { title: string; aside: string }) { return <div className="section-label"><strong>{title}</strong><span>{aside}</span></div>; }
function PanelEmpty({ title, text }: { title: string; text: string }) { return <div className="panel-empty"><span className="empty-pulse" /><strong>{title}</strong><p>{text}</p></div>; }
function time(value: string) { try { return new Date(value).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit', second: '2-digit' }); } catch { return '—'; } }

function panelTitle(tool: ToolId) { return { inspect: 'Inspector', responsive: 'Responsive Lab', console: 'Console', network: 'Network', ai: 'AI Critic', sessions: 'Local sessions', command: 'Command palette' }[tool]; }
function panelEyebrow(tool: ToolId) { return { inspect: 'X-RAY · STRUCTURE', responsive: 'VIEWPORT · LAYOUT', console: 'BEHAVIOR · TELEMETRY', network: 'REQUEST · EVIDENCE', ai: 'VISION · SEMANTICS', sessions: 'AUTO-DISCOVERY', command: 'LOCALVIEW' }[tool]; }

export function RailButton({ tool, active, onClick, children }: { tool: Exclude<ToolId, 'sessions' | 'command'>; active: boolean; onClick: () => void; children: ReactNode }) {
  const meta = toolMeta[tool];
  return <button className={`rail-button ${active ? 'active' : ''}`} onClick={onClick} aria-pressed={active} aria-label={meta.label}>{children}<span className="rail-tooltip">{meta.label}<kbd>{meta.shortcut}</kbd></span></button>;
}

export function CommandRailButton({ active, onClick }: { active: boolean; onClick: () => void }) {
  return <button className={`rail-button command ${active ? 'active' : ''}`} onClick={onClick} aria-label="Command palette"><CommandIcon /><span className="rail-tooltip">Command palette <kbd>⌘K</kbd></span></button>;
}
