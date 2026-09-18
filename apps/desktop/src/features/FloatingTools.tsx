import type { ReactNode } from 'react';
import type { DashboardState, LiveSessionState, ObserverEvent, Session } from '../types';
import { LOCALE_OPTIONS, translate, type MessageKey, type SupportedLocale } from '../i18n';
import type { LocalViewPreferences } from '../preferences';
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
  SettingsIcon,
  MoreIcon,
  SourceIcon,
  RulerIcon,
  CaptureIcon,
} from '../components/icons';

export type ToolId =
  | 'inspect'
  | 'responsive'
  | 'console'
  | 'network'
  | 'ai'
  | 'advanced'
  | 'settings'
  | 'sessions'
  | 'command';

export const toolMeta: Record<Exclude<ToolId, 'sessions' | 'command'>, { messageKey: MessageKey; shortcut: string }> = {
  inspect: { messageKey: 'tool.inspect', shortcut: 'I' },
  responsive: { messageKey: 'tool.responsive', shortcut: 'R' },
  console: { messageKey: 'tool.console', shortcut: 'C' },
  network: { messageKey: 'tool.network', shortcut: 'N' },
  ai: { messageKey: 'tool.ai', shortcut: 'A' },
  settings: { messageKey: 'tool.settings', shortcut: '⌘,' },
  advanced: { messageKey: 'tool.advanced', shortcut: 'M' },
};

interface FloatingPanelProps {
  tool: ToolId;
  state: DashboardState;
  live: LiveSessionState;
  current?: Session;
  url?: string;
  locale: SupportedLocale;
  preferences: LocalViewPreferences;
  onClose: () => void;
  onSelect: (id: string) => void;
  onOpenNative: () => void;
  onPause: () => void;
  onTool: (tool: ToolId) => void;
  onPreferencesChange: (patch: Partial<LocalViewPreferences>) => void;
  onResetWorkspace: () => void;
}

export function FloatingPanel({
  tool,
  state,
  live,
  current,
  url,
  locale,
  preferences,
  onClose,
  onSelect,
  onOpenNative,
  onPause,
  onTool,
  onPreferencesChange,
  onResetWorkspace,
}: FloatingPanelProps) {
  const bottomSheet = tool === 'console' || tool === 'network';
  const compact = tool === 'command';
  return (
    <section className={`floating-panel panel-${tool} ${bottomSheet ? 'bottom-sheet' : ''} ${compact ? 'command-panel' : ''}`} aria-label={`${tool} panel`}>
      <PanelHeader title={panelTitle(tool, locale)} eyebrow={panelEyebrow(tool)} onClose={onClose} />
      <div className="panel-body">
        {tool === 'inspect' && <Inspector current={current} live={live} onOpenNative={onOpenNative} locale={locale} />}
        {tool === 'advanced' && <AdvancedPanel current={current} live={live} onOpenNative={onOpenNative} />}
        {tool === 'settings' && (
          <SettingsPanel
            locale={locale}
            preferences={preferences}
            onPreferencesChange={onPreferencesChange}
            onResetWorkspace={onResetWorkspace}
          />
        )}
        {tool === 'responsive' && <ResponsivePanel current={current} />}
        {tool === 'console' && <ConsolePanel live={live} onOpenNative={onOpenNative} />}
        {tool === 'network' && <NetworkPanel current={current} live={live} onOpenNative={onOpenNative} />}
        {tool === 'ai' && <AiPanel current={current} locale={locale} />}
        {tool === 'sessions' && <SessionsPanel state={state} current={current} onSelect={onSelect} />}
        {tool === 'command' && (
          <CommandPanel
            state={state}
            current={current}
            url={url}
            locale={locale}
            preferences={preferences}
            onOpenNative={onOpenNative}
            onPause={onPause}
            onTool={onTool}
            onPreferencesChange={onPreferencesChange}
          />
        )}
      </div>
    </section>
  );
}

function PanelHeader({ title, eyebrow, onClose }: { title: string; eyebrow: string; onClose: () => void }) {
  return <div className="panel-header"><div><span>{eyebrow}</span><strong>{title}</strong></div><button className="close-button" aria-label={`Close ${title}`} onClick={onClose}><CloseIcon /></button></div>;
}

function Inspector({
  current,
  live,
  onOpenNative,
  locale,
}: {
  current?: Session;
  live: LiveSessionState;
  onOpenNative: () => void;
  locale: SupportedLocale;
}) {
  if (!current) {
    return <PanelEmpty title={translate(locale, 'empty.noTarget')} text={translate(locale, 'empty.runDevServer')} />;
  }
  const focused = [...live.observer].reverse().find((event) => event.kind === 'focus');
  const source = focused?.payload && typeof focused.payload.source === 'string'
    ? String(focused.payload.source)
    : undefined;

  return (
    <div className="inspector-stack human-inspector">
      <div className="inspector-hero">
        <div className="selection-cross"><InspectIcon /></div>
        <div>
          <span>{translate(locale, 'inspector.currentTarget')}</span>
          <strong>{focused?.reference ?? current.project.display_name}</strong>
          <p>{focused ? current.project.display_name : translate(locale, 'inspector.noSelection')}</p>
        </div>
      </div>

      <div className="quick-action-grid" aria-label="Inspector actions">
        <UnavailableInspectorAction
          icon={<SourceIcon />}
          label={translate(locale, 'action.openSource')}
          reason={source ? 'Source opening is not connected to this panel yet.' : translate(locale, 'inspector.sourceUnavailable')}
        />
        <UnavailableInspectorAction
          icon={<RulerIcon />}
          label={translate(locale, 'action.measure')}
          reason={focused ? 'Measurement is not connected to this panel yet.' : translate(locale, 'inspector.noSelection')}
        />
        <UnavailableInspectorAction
          icon={<CaptureIcon />}
          label={translate(locale, 'action.capture')}
          reason="Capture remains unavailable here until the panel can supply validated viewport authority."
        />
        <UnavailableInspectorAction
          icon={<SparkIcon />}
          label={translate(locale, 'action.askAi')}
          reason={focused ? translate(locale, 'ai.unavailable') : translate(locale, 'inspector.noSelection')}
        />
        <UnavailableInspectorAction
          icon={<ActivityIcon />}
          label={translate(locale, 'action.fix')}
          reason={focused ? translate(locale, 'ai.unavailable') : translate(locale, 'inspector.noSelection')}
        />
      </div>

      {!live.observer.length && <AttachNotice onOpenNative={onOpenNative} />}
    </div>
  );
}


function UnavailableInspectorAction({
  icon,
  label,
  reason,
}: {
  icon: ReactNode;
  label: string;
  reason: string;
}) {
  return (
    <button disabled aria-disabled="true" title={reason}>
      {icon}<span>{label}</span>
    </button>
  );
}

function AdvancedPanel({ current, live, onOpenNative }: { current?: Session; live: LiveSessionState; onOpenNative: () => void }) {
  if (!current) return <PanelEmpty title="No active target" text="Run a dev server to view diagnostics." />;
  const snapshot = [...live.observer].reverse().find((event) => event.kind === 'semantic_snapshot');
  const focused = [...live.observer].reverse().find((event) => event.kind === 'focus');
  const latest = live.observer.at(-1);
  return (
    <div className="inspector-stack diagnostics-stack">
      <InfoGrid rows={[
        ['Status', current.status],
        ['Observer', live.observer.length ? `${live.observer.length} events` : 'not attached'],
        ['Latest', latest?.kind ?? '—'],
        ['Focused ref', focused?.reference ?? '—'],
      ]} />
      {!live.observer.length && <AttachNotice onOpenNative={onOpenNative} />}
      {snapshot && <EvidenceCard event={snapshot} />}
      <div className="panel-section">
        <SectionLabel title="Project identity" aside="diagnostic" />
        <code className="path-block">{current.project.git_root ?? current.project.cwd ?? 'Process-derived project identity'}</code>
      </div>
      <div className="panel-section">
        <SectionLabel title="Runtime pipeline" aside="observer" />
        <div className="pipeline-list">
          <PipelineStep n="01" title="Semantic refs" state="ready" />
          <PipelineStep n="02" title="Geometry + layout evidence" state="ready" />
          <PipelineStep n="03" title="Source hints" state="ready" />
          <PipelineStep n="04" title="Secure observer drain" state={live.observer.length ? 'ready' : 'idle'} />
        </div>
      </div>
    </div>
  );
}

/* Language selector, Show target bar, Show tool rail */
function SettingsPanel({
  locale,
  preferences,
  onPreferencesChange,
  onResetWorkspace,
}: {
  locale: SupportedLocale;
  preferences: LocalViewPreferences;
  onPreferencesChange: (patch: Partial<LocalViewPreferences>) => void;
  onResetWorkspace: () => void;
}) {
  return (
    <div className="settings-panel">
      <section className="settings-section">
        <div className="settings-section-title">
          <strong>{translate(locale, 'settings.language')}</strong>
        </div>
        <label className="settings-row">
          <span>{translate(locale, 'settings.defaultLanguage')}</span>
          <select
            value={preferences.locale}
            onChange={(event) => onPreferencesChange({ locale: event.target.value as SupportedLocale })}
          >
            {LOCALE_OPTIONS.map((option) => (
              <option key={option.id} value={option.id}>
                {option.nativeLabel}{option.nativeLabel === option.label ? '' : ` · ${option.label}`}
              </option>
            ))}
          </select>
        </label>
      </section>

      <section className="settings-section">
        <div className="settings-section-title">
          <strong>{translate(locale, 'settings.workspace')}</strong>
        </div>
        <label className="settings-toggle">
          <span>{translate(locale, 'settings.showTargetBar')}</span>
          <input
            type="checkbox"
            checked={preferences.showTargetBar}
            onChange={(event) => onPreferencesChange({ showTargetBar: event.target.checked })}
          />
        </label>
        <label className="settings-toggle">
          <span>{translate(locale, 'settings.showToolRail')}</span>
          <input
            type="checkbox"
            checked={preferences.showToolRail}
            onChange={(event) => onPreferencesChange({ showToolRail: event.target.checked })}
          />
        </label>
        <label className="settings-toggle">
          <span>{translate(locale, 'settings.rememberChrome')}</span>
          <input
            type="checkbox"
            checked={preferences.rememberChromePositions}
            onChange={(event) => onPreferencesChange({ rememberChromePositions: event.target.checked })}
          />
        </label>
        <button className="settings-reset" onClick={onResetWorkspace}>
          {translate(locale, 'action.resetWorkspace')}
        </button>
      </section>
    </div>
  );
}

function ResponsivePanel({ current }: { current?: Session }) {
  const presets = [['Mobile S', '320', '568'], ['Mobile', '390', '844'], ['Tablet', '768', '1024'], ['Desktop', '1440', '900']];
  const unavailableReason = current
    ? 'Responsive viewport control is not connected to this panel yet.'
    : 'No active target';
  return <div>
    <div className="responsive-summary"><span>VIEWPORTS</span><strong>{current ? current.project.display_name : 'No target'}</strong></div>
    <div className="viewport-list">{presets.map(([name, width, height]) => <button key={name} disabled aria-disabled="true" title={unavailableReason}><span className="viewport-icon"/><div><strong>{name}</strong><span>{width} × {height}</span></div><kbd>{width}</kbd></button>)}</div>
    <div className="panel-note">Viewport tools open only when needed.</div>
  </div>;
}

function ConsolePanel({ live, onOpenNative }: { live: LiveSessionState; onOpenNative: () => void }) {
  const events = live.observer.filter((event) => event.kind === 'console' || event.kind === 'runtime_error').slice(-80);
  return <div className="stream-panel">
    <div className="stream-toolbar"><span className="filter-chip active">Live</span><span className="stream-status"><i className={events.length ? '' : 'muted'} /> {events.length} event{events.length === 1 ? '' : 's'}</span></div>
    {events.length ? <div className="evidence-stream">{events.map((event) => <ConsoleRow key={`${event.seq}-${event.captured_at}`} event={event} />)}</div> : <EmptyEvidence icon={<ConsoleIcon />} title="No console events" text="Open the preview to collect console events." action="Open preview" onAction={onOpenNative} />}
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
    {events.length ? <div className="evidence-stream network-stream">{events.map((event) => <NetworkRow key={`${event.seq}-${event.captured_at}`} event={event} />)}</div> : <EmptyEvidence icon={<NetworkIcon />} title="No network events" text="Open the preview to collect request metadata." action="Open preview" onAction={onOpenNative} />}
  </div>;
}

function AiPanel({ current, locale }: { current?: Session; locale: SupportedLocale }) {
  const unavailableReason = current ? translate(locale, 'ai.unavailable') : translate(locale, 'empty.noTarget');
  return <div className="ai-panel-content human-ai-panel">
    <div className="ai-mark"><SparkIcon /></div>
    <h2>AI</h2>
    <div className="suggestion-grid">
      <button disabled aria-disabled="true" title={unavailableReason}>{translate(locale, 'ai.askSelection')}</button>
      <button disabled aria-disabled="true" title={unavailableReason}>{translate(locale, 'ai.explainIssue')}</button>
      <button disabled aria-disabled="true" title={unavailableReason}>{translate(locale, 'ai.fixSelection')}</button>
      <button disabled aria-disabled="true" title={unavailableReason}>{translate(locale, 'ai.verifyChange')}</button>
    </div>
    <span className="compact-status">{translate(locale, 'ai.unavailable')}</span>
  </div>;
}

function SessionsPanel({ state, current, onSelect }: { state: DashboardState; current?: Session; onSelect: (id: string) => void }) {
  return <div className="sessions-panel"><div className="session-overview"><strong>{state.sessions.length}</strong><span>detected localhost session{state.sessions.length === 1 ? '' : 's'}</span></div><div className="session-cards">
    {state.sessions.map((session) => <button key={session.id} className={current?.id === session.id ? 'selected' : ''} onClick={() => onSelect(session.id)}><span className={`health-dot ${session.status === 'disconnected' ? 'danger' : session.status === 'hidden' ? 'warn' : ''}`} /><div><strong>{session.project.display_name}</strong><span>{session.classification.framework ?? 'Web'} · :{session.endpoint.port}</span></div><span className="session-state">{session.status}</span></button>)}
    {!state.sessions.length && <PanelEmpty title="No app detected" text="Run your dev server to begin." />}
  </div></div>;
}

function CommandPanel({
  state,
  current,
  url,
  locale,
  preferences,
  onOpenNative,
  onPause,
  onTool,
  onPreferencesChange,
}: {
  state: DashboardState;
  current?: Session;
  url?: string;
  locale: SupportedLocale;
  preferences: LocalViewPreferences;
  onOpenNative: () => void;
  onPause: () => void;
  onTool: (tool: ToolId) => void;
  onPreferencesChange: (patch: Partial<LocalViewPreferences>) => void;
}) {
  const commands = [
    { icon: <InspectIcon />, title: translate(locale, 'tool.inspect'), detail: current?.project.display_name ?? '', keys: 'I', action: () => onTool('inspect'), disabled: !current },
    { icon: <ResponsiveIcon />, title: translate(locale, 'tool.responsive'), detail: '', keys: 'R', action: () => onTool('responsive'), disabled: !current },
    { icon: <ExternalIcon />, title: translate(locale, 'action.openPreview'), detail: url ?? '', keys: '↵', action: onOpenNative, disabled: !current },
    { icon: <SettingsIcon />, title: translate(locale, 'tool.settings'), detail: '', keys: '⌘,', action: () => onTool('settings') },
    { icon: <MoreIcon />, title: translate(locale, 'tool.advanced'), detail: '', keys: 'M', action: () => onTool('advanced'), disabled: !current },
    {
      icon: <InspectIcon />,
      title: preferences.showTargetBar ? translate(locale, 'action.hideTargetBar') : translate(locale, 'action.showTargetBar'),
      detail: '',
      keys: '⇧⌃T',
      action: () => onPreferencesChange({ showTargetBar: !preferences.showTargetBar }),
    },
    {
      icon: <CommandIcon />,
      title: preferences.showToolRail ? translate(locale, 'action.hideToolRail') : translate(locale, 'action.showToolRail'),
      detail: '',
      keys: '',
      action: () => onPreferencesChange({ showToolRail: !preferences.showToolRail }),
    },
    {
      icon: state.health.paused ? <PlayIcon /> : <PauseIcon />,
      title: state.health.paused ? translate(locale, 'action.resumeDiscovery') : translate(locale, 'action.pauseDiscovery'),
      detail: '',
      keys: 'P',
      action: onPause,
    },
  ];
  return <div className="command-content">
    <div className="command-search"><SearchIcon /><input autoFocus placeholder="Type a command…" aria-label="Search commands" /><kbd>ESC</kbd></div>
    <div className="command-list">{commands.map((command) => <button key={command.title} onClick={command.action} disabled={command.disabled}><span className="command-icon">{command.icon}</span><div><strong>{command.title}</strong>{command.detail && <span>{command.detail}</span>}</div>{command.keys && <kbd>{command.keys}</kbd>}</button>)}</div>
    <div className="command-footer"><span>LocalView v{state.health.version}</span></div>
  </div>;
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

function panelTitle(tool: ToolId, locale: SupportedLocale) {
  return {
    inspect: translate(locale, 'inspector.title'),
    responsive: translate(locale, 'tool.responsive'),
    console: translate(locale, 'tool.console'),
    network: translate(locale, 'tool.network'),
    ai: translate(locale, 'tool.ai'),
    advanced: translate(locale, 'advanced.title'),
    settings: translate(locale, 'settings.title'),
    sessions: 'Sessions',
    command: translate(locale, 'tool.command'),
  }[tool];
}
function panelEyebrow(tool: ToolId) {
  return {
    inspect: 'TOOLS',
    responsive: 'TOOLS',
    console: 'TOOLS',
    network: 'TOOLS',
    ai: 'TOOLS',
    advanced: 'DIAGNOSTICS',
    settings: 'PREFERENCES',
    sessions: 'SESSIONS',
    command: 'LOCALVIEW',
  }[tool];
}

export function RailButton({
  tool,
  active,
  onClick,
  children,
  locale,
}: {
  tool: Exclude<ToolId, 'sessions' | 'command'>;
  active: boolean;
  onClick: () => void;
  children: ReactNode;
  locale: SupportedLocale;
}) {
  const meta = toolMeta[tool];
  const label = translate(locale, meta.messageKey);
  return <button className={`rail-button ${active ? 'active' : ''}`} onClick={onClick} aria-pressed={active} aria-label={label}>{children}<span className="rail-tooltip">{label}<kbd>{meta.shortcut}</kbd></span></button>;
}

export function CommandRailButton({ active, onClick, locale }: { active: boolean; onClick: () => void; locale: SupportedLocale }) {
  const label = translate(locale, 'tool.command');
  return <button className={`rail-button command ${active ? 'active' : ''}`} onClick={onClick} aria-label={label}><CommandIcon /><span className="rail-tooltip">{label} <kbd>⌘K</kbd></span></button>;
}
