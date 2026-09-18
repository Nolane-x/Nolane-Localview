import { useCallback, useEffect, useMemo, useRef, useState, type ReactNode } from 'react';
import { api } from '../api';
import { COMMAND_IDS, type CommandId } from '../commands';
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
  type HumanCaptureState,
  type HumanMeasureState,
  type HumanSourceOpenState,
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

export default function LocalViewShell() {
  const [state, setState] = useState<DashboardState>(fallback);
  const [selected, setSelected] = useState<string>();
  const [activeTool, setActiveTool] = useState<ToolId>();
  const [error, setError] = useState<string>();
  const [live, setLive] = useState<LiveSessionState>(emptyLive);
  const [immersive, setImmersive] = useState(false);
  const [preferences, setPreferences] = useState<LocalViewPreferences>(() => loadPreferences());
  const [captureState, setCaptureState] = useState<HumanCaptureState>({ status: 'idle' });
  const [measureState, setMeasureState] = useState<HumanMeasureState>({ status: 'idle' });
  const [sourceOpenState, setSourceOpenState] = useState<HumanSourceOpenState>({ status: 'idle' });
  const captureInFlight = useRef(false);
  const captureGeneration = useRef(0);
  const measureInFlight = useRef(false);
  const measureGeneration = useRef(0);
  const sourceOpenInFlight = useRef(false);
  const sourceOpenGeneration = useRef(0);
  const selectedReferenceRef = useRef<string | undefined>(undefined);

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

  const selectedReference = useMemo(
    () => [...live.observer]
      .reverse()
      .find(
        (event) =>
          event.kind === 'focus'
          && typeof event.reference === 'string'
          && /^@e[0-9a-f]+$/i.test(event.reference),
      )
      ?.reference,
    [live.observer],
  );

  useEffect(() => {
    selectedReferenceRef.current = selectedReference;
    measureGeneration.current += 1;
    measureInFlight.current = false;
    setMeasureState({ status: 'idle' });
    sourceOpenGeneration.current += 1;
    sourceOpenInFlight.current = false;
    setSourceOpenState({ status: 'idle' });
  }, [selectedReference]);

  useEffect(() => {
    captureGeneration.current += 1;
    captureInFlight.current = false;
    setCaptureState({ status: 'idle' });
    measureGeneration.current += 1;
    measureInFlight.current = false;
    selectedReferenceRef.current = undefined;
    setMeasureState({ status: 'idle' });
    sourceOpenGeneration.current += 1;
    sourceOpenInFlight.current = false;
    setSourceOpenState({ status: 'idle' });
  }, [current?.id]);

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

  const togglePause = useCallback(async () => {
    try {
      state.health.paused ? await api.resume() : await api.pause();
      await refresh();
    } catch (cause) {
      setError(String(cause));
    }
  }, [refresh, state.health.paused]);

  const openNative = useCallback(async (session: Session | undefined = current) => {
    if (!session) return;
    try {
      await api.openPreview(
        session.id,
        `${session.endpoint.scheme}://${session.endpoint.host}:${session.endpoint.port}/`,
        session.project.display_name,
      );
    } catch (cause) {
      setError(String(cause));
    }
  }, [current]);

  const captureCurrentViewport = useCallback(async () => {
    const session = current;
    if (!session || captureInFlight.current) return;

    captureInFlight.current = true;
    const generation = ++captureGeneration.current;
    setCaptureState({ status: 'capturing' });

    try {
      const receipt = await api.captureCurrentViewport(session.id);
      if (generation !== captureGeneration.current) return;
      setCaptureState({
        status: 'success',
        evidenceId: receipt.evidence_id,
        pixelWidth: receipt.pixel_width,
        pixelHeight: receipt.pixel_height,
        backend: receipt.backend,
      });
    } catch (cause) {
      if (generation !== captureGeneration.current) return;
      const detail = String(cause).toLowerCase();
      const unavailable =
        detail.includes('managed surface') ||
        detail.includes('surface is unavailable') ||
        detail.includes('no localview-managed') ||
        detail.includes('open preview');
      setCaptureState({
        status: 'failure',
        reason: unavailable ? 'unavailable' : 'failed',
      });
    } finally {
      if (generation === captureGeneration.current) {
        captureInFlight.current = false;
      }
    }
  }, [current]);

  const measureCurrentSelection = useCallback(async (reference: string) => {
    const session = current;
    if (!session || !reference || measureInFlight.current) return;

    measureInFlight.current = true;
    const generation = ++measureGeneration.current;
    selectedReferenceRef.current = reference;
    setMeasureState({ status: 'measuring', reference });

    try {
      const receipt = await api.measureElement(session.id, reference);
      if (
        generation !== measureGeneration.current
        || reference !== selectedReferenceRef.current
      ) {
        return;
      }
      setMeasureState({
        status: 'success',
        reference,
        width: receipt.rect.width,
        height: receipt.rect.height,
        x: receipt.rect.x,
        y: receipt.rect.y,
      });
    } catch {
      if (
        generation !== measureGeneration.current
        || reference !== selectedReferenceRef.current
      ) {
        return;
      }
      setMeasureState({ status: 'failure', reason: 'failed' });
    } finally {
      if (generation === measureGeneration.current) {
        measureInFlight.current = false;
      }
    }
  }, [current]);

  const openSourceForSelection = useCallback(async (reference: string) => {
    const session = current;
    if (!session || !reference || sourceOpenInFlight.current) return;

    const sourceOpenReference = reference;
    const sourceOpenRequest = {
      sessionId: session.id,
      reference: sourceOpenReference,
    };

    sourceOpenInFlight.current = true;
    const generation = ++sourceOpenGeneration.current;
    selectedReferenceRef.current = sourceOpenReference;
    setSourceOpenState({ status: 'opening', reference: sourceOpenReference });

    try {
      const receipt = await api.openSourceForSelection(sourceOpenRequest);
      if (
        generation !== sourceOpenGeneration.current
        || sourceOpenReference !== selectedReferenceRef.current
      ) {
        return;
      }
      setSourceOpenState({
        status: 'success',
        reference: sourceOpenReference,
        displayFile: receipt.displayFile,
        line: receipt.line,
        column: receipt.column ?? undefined,
      });
    } catch (cause) {
      if (
        generation !== sourceOpenGeneration.current
        || sourceOpenReference !== selectedReferenceRef.current
      ) {
        return;
      }
      const detail = String(cause).toLowerCase();
      const reason = detail.includes('launcher')
        ? 'launcher_unavailable'
        : detail.includes('mapping')
          || detail.includes('source file')
          || detail.includes('project root')
          || detail.includes('selection')
          || detail.includes('outside project')
          || detail.includes('path traversal')
          || detail.includes('symlink')
            ? 'unavailable'
            : 'failed';
      setSourceOpenState({ status: 'failure', reference: sourceOpenReference, reason });
    } finally {
      if (generation === sourceOpenGeneration.current) {
        sourceOpenInFlight.current = false;
      }
    }
  }, [current]);

  const executeCommand = useCallback((command: CommandId) => {
    switch (command) {
      case COMMAND_IDS.inspectActivate:
        setActiveTool('inspect');
        return;
      case COMMAND_IDS.sourceOpen:
        setActiveTool('inspect');
        if (selectedReference) void openSourceForSelection(selectedReference);
        return;
      case COMMAND_IDS.responsiveOpen:
        setActiveTool('responsive');
        return;
      case COMMAND_IDS.consoleOpen:
        setActiveTool('console');
        return;
      case COMMAND_IDS.networkOpen:
        setActiveTool('network');
        return;
      case COMMAND_IDS.aiOpen:
        setActiveTool('ai');
        return;
      case COMMAND_IDS.advancedOpen:
        setActiveTool('advanced');
        return;
      case COMMAND_IDS.settingsOpen:
        setActiveTool('settings');
        return;
      case COMMAND_IDS.previewOpenNative:
        void openNative();
        return;
      case COMMAND_IDS.workspaceToggleTargetBar:
        patchPreferences({ showTargetBar: !preferences.showTargetBar });
        return;
      case COMMAND_IDS.workspaceToggleToolRail:
        patchPreferences({ showToolRail: !preferences.showToolRail });
        return;
      case COMMAND_IDS.workspaceToggleChrome: {
        const anyVisible = preferences.showTargetBar || preferences.showToolRail;
        patchPreferences({ showTargetBar: !anyVisible, showToolRail: !anyVisible });
        return;
      }
      case COMMAND_IDS.workspaceResetLayout:
        resetWorkspacePreferences();
        return;
      case COMMAND_IDS.sessionPauseDiscovery:
        void togglePause();
        return;
      default:
        return;
    }
  }, [
    openNative,
    openSourceForSelection,
    patchPreferences,
    preferences.showTargetBar,
    preferences.showToolRail,
    resetWorkspacePreferences,
    selectedReference,
    togglePause,
  ]);

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
        executeCommand(COMMAND_IDS.settingsOpen);
        return;
      }
      if (event.ctrlKey && event.shiftKey && event.key.toLowerCase() === 't') {
        event.preventDefault();
        void TOGGLE_TARGET_BAR_SHORTCUT;
        executeCommand(COMMAND_IDS.workspaceToggleTargetBar);
        return;
      }
      const shortcuts: Record<string, CommandId> = {
        i: COMMAND_IDS.inspectActivate,
        r: COMMAND_IDS.responsiveOpen,
        c: COMMAND_IDS.consoleOpen,
        n: COMMAND_IDS.networkOpen,
        a: COMMAND_IDS.aiOpen,
        m: COMMAND_IDS.advancedOpen,
        p: COMMAND_IDS.sessionPauseDiscovery,
      };
      const command = shortcuts[event.key.toLowerCase()];
      if (command) executeCommand(command);
    };
    window.addEventListener('keydown', onKeyDown);
    return () => window.removeEventListener('keydown', onKeyDown);
  }, [executeCommand, toggleTool]);

  return (
    <div className={`localview ${immersive ? 'is-immersive' : ''} ${preferences.reducedMotion === 'reduce' ? 'is-reduced-motion' : ''}`}>
      <WorkspaceSurface current={current} url={currentUrl} support={state.workspace_surface} locale={preferences.locale} />
      <div className="chrome-layer" aria-label={translate(preferences.locale, 'aria.localViewControls')}>
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
          <FloatingRail
            activeTool={activeTool}
            locale={preferences.locale}
            onTool={toggleTool}
            onCommand={() => toggleTool('command')}
          />
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
            captureState={captureState}
            onCapture={() => void captureCurrentViewport()}
            sourceOpenState={sourceOpenState}
            onOpenSource={(reference) => void openSourceForSelection(reference)}
            measureState={measureState}
            onMeasure={(reference) => void measureCurrentSelection(reference)}
            onCommand={executeCommand}
            onPreferencesChange={patchPreferences}
            onResetWorkspace={resetWorkspacePreferences}
          />
        )}
        {error && <RuntimeToast locale={preferences.locale} onRetry={() => void refresh()} />}
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
    <button className="logo-button" aria-label={translate(locale, 'action.showSessions')} onClick={onSessions}><span className="logo-glyph">L</span></button>
    <div className="top-divider"/>
    <div className="target-block">
      <div className="target-row">
        <span className={`health-dot ${state.health.paused ? 'warn' : ''}`}/>
        <select
          aria-label={translate(locale, 'aria.currentSession')}
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
      <IconButton label={translate(locale, 'action.immersive')} onClick={onImmersive}><ExpandIcon/></IconButton>
      <IconButton label={translate(locale, 'action.hideTargetBar')} onClick={onHideTargetBar}><HideIcon/></IconButton>
    </div>
  </header>;
}

function FloatingRail({
  activeTool,
  locale,
  onTool,
  onCommand,
}: {
  activeTool?: ToolId;
  locale: LocalViewPreferences['locale'];
  onTool: (tool: ToolId) => void;
  onCommand: () => void;
}) {
  return <nav className="floating-rail" aria-label={translate(locale, 'aria.localViewTools')}>
    <RailButton tool="inspect" locale={locale} active={activeTool === 'inspect'} onClick={() => onTool('inspect')}><InspectIcon/></RailButton>
    <RailButton tool="responsive" locale={locale} active={activeTool === 'responsive'} onClick={() => onTool('responsive')}><ResponsiveIcon/></RailButton>
    <RailButton tool="console" locale={locale} active={activeTool === 'console'} onClick={() => onTool('console')}><ConsoleIcon/></RailButton>
    <RailButton tool="network" locale={locale} active={activeTool === 'network'} onClick={() => onTool('network')}><NetworkIcon/></RailButton>
    <div className="rail-divider"/>
    <RailButton tool="ai" locale={locale} active={activeTool === 'ai'} onClick={() => onTool('ai')}><SparkIcon/></RailButton>
    <RailButton tool="settings" locale={locale} active={activeTool === 'settings'} onClick={() => onTool('settings')}><SettingsIcon/></RailButton>
    <RailButton tool="advanced" locale={locale} active={activeTool === 'advanced'} onClick={() => onTool('advanced')}><MoreIcon/></RailButton>
    <CommandRailButton locale={locale} active={activeTool === 'command'} onClick={onCommand}/>
  </nav>;
}

function RuntimeToast({ locale, onRetry }: { locale: LocalViewPreferences['locale']; onRetry: () => void }) {
  return <div className="runtime-toast" role="status" aria-live="polite"><span className="health-dot danger"/><div><strong>{translate(locale, 'runtime.unavailable')}</strong><span>{translate(locale, 'runtime.unavailableHint')}</span></div><button onClick={onRetry}>{translate(locale, 'action.retry')}</button></div>;
}

function IconButton({ label, onClick, disabled, children }: { label: string; onClick: () => void; disabled?: boolean; children: ReactNode }) {
  return <button className="icon-button" aria-label={label} title={label} onClick={onClick} disabled={disabled}>{children}</button>;
}

export function LocalViewCommandGlyph() { return <CommandIcon/>; }
