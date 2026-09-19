import { useState, type ReactNode } from 'react';
import { COMMAND_IDS, type CommandId } from '../commands';
import type { AiFixCapability, AiProviderCapability, VerifyScope, VerifyStatus } from '../api';
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

export type HumanCaptureState =
  | { status: 'idle' }
  | { status: 'capturing' }
  | {
      status: 'success';
      evidenceId: string;
      pixelWidth: number;
      pixelHeight: number;
      backend: string;
    }
  | { status: 'failure'; reason: 'unavailable' | 'failed' };

export type HumanSourceOpenState =
  | { status: 'idle' }
  | { status: 'opening'; reference: string }
  | {
      status: 'success';
      reference: string;
      displayFile: string;
      line: number;
      column?: number;
    }
  | {
      status: 'failure';
      reference: string;
      reason: 'unavailable' | 'launcher_unavailable' | 'failed';
    };

export type HumanMeasureState =
  | { status: 'idle' }
  | { status: 'measuring'; reference: string }
  | {
      status: 'success';
      reference: string;
      width: number;
      height: number;
      x: number;
      y: number;
    }
  | { status: 'failure'; reason: 'failed' | 'unavailable' };

export type HumanAskAiState =
  | { status: 'idle' }
  | { status: 'asking'; reference: string; question: string }
  | {
      status: 'success';
      reference: string;
      question: string;
      answer: string;
      providerLabel: string;
      snapshotVersion: number;
    }
  | {
      status: 'failure';
      reference?: string;
      reason: 'provider_unavailable' | 'context_unavailable' | 'invalid_question' | 'question_too_long' | 'failed';
    };

export type HumanFixState =
  | { status: 'idle' }
  | { status: 'disclosure'; reference: string }
  | { status: 'proposing'; reference: string; instruction: string }
  | {
      status: 'proposal';
      proposalId: string;
      reference: string;
      instruction: string;
      displayFile: string;
      summary: string;
      diff: string;
      providerLabel: string;
      expiresAtUnixMs: number;
    }
  | {
      status: 'applying';
      proposalId: string;
      reference: string;
      displayFile: string;
    }
  | {
      status: 'success';
      displayFile: string;
      changedStartLine: number;
      changedEndLine: number;
    }
  | {
      status: 'failure';
      reference?: string;
      reason:
        | 'provider_unavailable'
        | 'source_unavailable'
        | 'invalid_instruction'
        | 'instruction_too_long'
        | 'sensitive_source'
        | 'unsupported_source'
        | 'proposal_invalid'
        | 'proposal_expired'
        | 'source_changed'
        | 'apply_failed'
        | 'failed';
    };

export type HumanVerifyState =
  | { status: 'idle' }
  | {
      status: 'ready';
      verificationId: string;
      reference: string;
      displayFile: string;
      scope: VerifyScope;
    }
  | {
      status: 'verifying';
      verificationId: string;
      reference: string;
      displayFile: string;
      scope: VerifyScope;
    }
  | {
      status: 'success';
      verificationId: string;
      reference: string;
      displayFile: string;
      scope: VerifyScope;
      result: VerifyStatus;
      semanticChanges: string[];
      regressionSignals: string[];
      viewportChangedRatio?: number | null;
      targetChangedRatio?: number | null;
      providerLabel?: string | null;
      advisorySummary?: string | null;
    }
  | {
      status: 'failure';
      verificationId?: string;
      reference?: string;
      displayFile?: string;
      scope?: VerifyScope;
      reason: 'expired' | 'source_changed' | 'route_changed' | 'target_unavailable' | 'settle_failed' | 'failed';
    };

function verifyCanRetry(state: HumanVerifyState): boolean {
  return state.status === 'failure'
    && (state.reason === 'settle_failed' || state.reason === 'failed')
    && !!state.verificationId
    && !!state.reference
    && !!state.displayFile
    && !!state.scope;
}

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
  captureState: HumanCaptureState;
  onCapture: () => void;
  sourceOpenState: HumanSourceOpenState;
  selectedReference?: string;
  onOpenSource: (reference: string) => void;
  measureState: HumanMeasureState;
  onMeasure: (reference: string) => void;
  aiProviderCapability: AiProviderCapability;
  askAiState: HumanAskAiState;
  onAskAi: (question: string) => void;
  onRefreshAiProvider: () => void;
  fixCapability: AiFixCapability;
  fixState: HumanFixState;
  verifyState: HumanVerifyState;
  onVerifyChange: () => void;
  onBeginFix: () => void;
  onPrepareFix: (instruction: string) => void;
  onApplyFix: () => void;
  onDiscardFix: () => void;
  onRefreshFixCapability: () => void;
  onCommand: (command: CommandId) => void;
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
  captureState,
  onCapture,
  sourceOpenState,
  selectedReference,
  onOpenSource,
  measureState,
  onMeasure,
  aiProviderCapability,
  askAiState,
  onAskAi,
  onRefreshAiProvider,
  fixCapability,
  fixState,
  verifyState,
  onVerifyChange,
  onBeginFix,
  onPrepareFix,
  onApplyFix,
  onDiscardFix,
  onRefreshFixCapability,
  onCommand,
  onPreferencesChange,
  onResetWorkspace,
}: FloatingPanelProps) {
  const bottomSheet = tool === 'console' || tool === 'network';
  const compact = tool === 'command';
  return (
    <section className={`floating-panel panel-${tool} ${bottomSheet ? 'bottom-sheet' : ''} ${compact ? 'command-panel' : ''}`} aria-label={panelTitle(tool, locale)}>
      <PanelHeader title={panelTitle(tool, locale)} eyebrow={panelEyebrow(tool, locale)} locale={locale} onClose={onClose} />
      <div className="panel-body">
        {tool === 'inspect' && (
          <Inspector
            current={current}
            live={live}
            onOpenNative={onOpenNative}
            locale={locale}
            captureState={captureState}
            onCapture={onCapture}
            sourceOpenState={sourceOpenState}
            selectedReference={selectedReference}
            onOpenSource={onOpenSource}
            measureState={measureState}
            onMeasure={onMeasure}
            aiProviderCapability={aiProviderCapability}
            askAiState={askAiState}
            onAskAi={onAskAi}
            fixCapability={fixCapability}
            fixState={fixState}
            verifyState={verifyState}
            onVerifyChange={onVerifyChange}
            onBeginFix={onBeginFix}
          />
        )}
        {tool === 'advanced' && <AdvancedPanel current={current} live={live} locale={locale} onOpenNative={onOpenNative} />}
        {tool === 'settings' && (
          <SettingsPanel
            locale={locale}
            preferences={preferences}
            onPreferencesChange={onPreferencesChange}
            onResetWorkspace={onResetWorkspace}
          />
        )}
        {tool === 'responsive' && <ResponsivePanel current={current} locale={locale} />}
        {tool === 'console' && <ConsolePanel live={live} locale={locale} onOpenNative={onOpenNative} />}
        {tool === 'network' && <NetworkPanel current={current} live={live} locale={locale} onOpenNative={onOpenNative} />}
        {tool === 'ai' && (
          <AiPanel
            current={current}
            locale={locale}
            selectedReference={selectedReference}
            providerCapability={aiProviderCapability}
            askAiState={askAiState}
            onAskAi={onAskAi}
            onRefreshProvider={onRefreshAiProvider}
            fixCapability={fixCapability}
            fixState={fixState}
            verifyState={verifyState}
            onVerifyChange={onVerifyChange}
            onBeginFix={onBeginFix}
            onPrepareFix={onPrepareFix}
            onApplyFix={onApplyFix}
            onDiscardFix={onDiscardFix}
            onRefreshFixCapability={onRefreshFixCapability}
          />
        )}
        {tool === 'sessions' && <SessionsPanel state={state} current={current} locale={locale} onSelect={onSelect} />}
        {tool === 'command' && (
          <CommandPanel
            state={state}
            current={current}
            url={url}
            locale={locale}
            preferences={preferences}
            selectedReference={selectedReference}
            providerCapability={aiProviderCapability}
            fixCapability={fixCapability}
            verifyState={verifyState}
            onCommand={onCommand}
          />
        )}
      </div>
    </section>
  );
}

function PanelHeader({ title, eyebrow, locale, onClose }: { title: string; eyebrow: string; locale: SupportedLocale; onClose: () => void }) {
  return <div className="panel-header"><div><span>{eyebrow}</span><strong>{title}</strong></div><button className="close-button" aria-label={`${translate(locale, 'action.close')} ${title}`} onClick={onClose}><CloseIcon /></button></div>;
}

function sourceOpenFailureMessage(
  locale: SupportedLocale,
  reason: Extract<HumanSourceOpenState, { status: 'failure' }>['reason'],
): string {
  switch (reason) {
    case 'launcher_unavailable':
      return translate(locale, 'source.launcherUnavailable');
    case 'unavailable':
      return translate(locale, 'source.unavailable');
    case 'failed':
    default:
      return translate(locale, 'source.failed');
  }
}

function askAiFailureMessage(
  locale: SupportedLocale,
  reason: Extract<HumanAskAiState, { status: 'failure' }>['reason'],
): string {
  switch (reason) {
    case 'provider_unavailable':
      return translate(locale, 'ai.unavailable');
    case 'context_unavailable':
      return translate(locale, 'ai.contextUnavailable');
    case 'invalid_question':
      return translate(locale, 'ai.enterQuestion');
    case 'question_too_long':
      return translate(locale, 'ai.questionTooLong');
    case 'failed':
    default:
      return translate(locale, 'ai.failed');
  }
}

function fixFailureMessage(
  locale: SupportedLocale,
  reason: Extract<HumanFixState, { status: 'failure' }>['reason'],
): string {
  switch (reason) {
    case 'provider_unavailable':
      return translate(locale, 'fix.unavailable');
    case 'source_unavailable':
      return translate(locale, 'fix.sourceUnavailable');
    case 'invalid_instruction':
      return translate(locale, 'fix.enterInstruction');
    case 'instruction_too_long':
      return translate(locale, 'fix.instructionTooLong');
    case 'sensitive_source':
      return translate(locale, 'fix.sensitiveSource');
    case 'unsupported_source':
      return translate(locale, 'fix.unsupportedSource');
    case 'proposal_invalid':
      return translate(locale, 'fix.proposalInvalid');
    case 'proposal_expired':
      return translate(locale, 'fix.expired');
    case 'source_changed':
      return translate(locale, 'fix.sourceChanged');
    case 'apply_failed':
      return translate(locale, 'fix.applyFailed');
    case 'failed':
    default:
      return translate(locale, 'fix.failed');
  }
}

function Inspector({
  current,
  live,
  onOpenNative,
  locale,
  captureState,
  onCapture,
  sourceOpenState,
  selectedReference,
  onOpenSource,
  measureState,
  onMeasure,
  aiProviderCapability,
  askAiState,
  onAskAi,
  fixCapability,
  fixState,
  onBeginFix,
}: {
  current?: Session;
  live: LiveSessionState;
  onOpenNative: () => void;
  locale: SupportedLocale;
  captureState: HumanCaptureState;
  onCapture: () => void;
  sourceOpenState: HumanSourceOpenState;
  selectedReference?: string;
  onOpenSource: (reference: string) => void;
  measureState: HumanMeasureState;
  onMeasure: (reference: string) => void;
  aiProviderCapability: AiProviderCapability;
  askAiState: HumanAskAiState;
  onAskAi: (question: string) => void;
  fixCapability: AiFixCapability;
  fixState: HumanFixState;
  verifyState: HumanVerifyState;
  onVerifyChange: () => void;
  onBeginFix: () => void;
}) {
  const focused = [...live.observer].reverse().find((event) => event.kind === 'focus');
  const measureReference = selectedReference;
  const captureBusy = captureState.status === 'capturing';
  const sourceOpenBusy = sourceOpenState.status === 'opening';
  const measureBusy = measureState.status === 'measuring';
  const askAiBusy = askAiState.status === 'asking';
  const fixBusy = fixState.status === 'proposing' || fixState.status === 'applying';

  return (
    <div className="inspector-stack human-inspector">
      {current ? (
        <div className="inspector-hero">
          <div className="selection-cross"><InspectIcon /></div>
          <div>
            <span>{translate(locale, 'inspector.currentTarget')}</span>
            <strong>{focused?.reference ?? current.project.display_name}</strong>
            <p>{focused ? current.project.display_name : translate(locale, 'inspector.noSelection')}</p>
          </div>
        </div>
      ) : (
        <PanelEmpty title={translate(locale, 'empty.noTarget')} text={translate(locale, 'empty.runDevServer')} />
      )}

      <div className="quick-action-grid" aria-label={translate(locale, 'aria.inspectorActions')}>
        <button
          className="source-open-action"
          onClick={() => measureReference && onOpenSource(measureReference)}
          disabled={!current || !measureReference || sourceOpenBusy}
          aria-busy={sourceOpenBusy}
          title={
            !current
              ? translate(locale, 'source.unavailable')
              : !measureReference
                ? translate(locale, 'source.selectFirst')
                : translate(locale, 'action.openSource')
          }
        >
          <SourceIcon />
          <span>{sourceOpenBusy ? translate(locale, 'source.opening') : translate(locale, 'action.openSource')}</span>
        </button>
        <button
          className="measure-action"
          onClick={() => measureReference && onMeasure(measureReference)}
          disabled={!current || !measureReference || measureBusy}
          aria-busy={measureBusy}
          title={
            !current
              ? translate(locale, 'measure.unavailable')
              : !measureReference
                ? translate(locale, 'measure.selectFirst')
                : translate(locale, 'action.measure')
          }
        >
          <RulerIcon />
          <span>{measureBusy ? translate(locale, 'measure.inProgress') : translate(locale, 'action.measure')}</span>
        </button>
        <button
          className="capture-action"
          onClick={onCapture}
          disabled={!current || captureBusy}
          aria-busy={captureBusy}
          title={!current ? translate(locale, 'capture.unavailable') : translate(locale, 'action.capture')}
        >
          <CaptureIcon />
          <span>{captureBusy ? translate(locale, 'capture.inProgress') : translate(locale, 'action.capture')}</span>
        </button>
        <button
          className="ask-ai-action"
          onClick={() => onAskAi(translate(locale, 'ai.defaultQuestion'))}
          disabled={!current || !measureReference || !aiProviderCapability.available || askAiBusy}
          aria-busy={askAiBusy}
          title={
            !current
              ? translate(locale, 'empty.noTarget')
              : !measureReference
                ? translate(locale, 'inspector.noSelection')
                : !aiProviderCapability.available
                  ? translate(locale, 'ai.unavailable')
                  : translate(locale, 'action.askAi')
          }
        >
          <SparkIcon />
          <span>{askAiBusy ? translate(locale, 'ai.asking') : translate(locale, 'action.askAi')}</span>
        </button>
        <button
          className="fix-action"
          onClick={onBeginFix}
          disabled={!current || !measureReference || !fixCapability.available || fixBusy}
          aria-busy={fixBusy}
          title={
            !current
              ? translate(locale, 'empty.noTarget')
              : !measureReference
                ? translate(locale, 'inspector.noSelection')
                : !fixCapability.available
                  ? translate(locale, 'fix.unavailable')
                  : translate(locale, 'action.fix')
          }
        >
          <ActivityIcon />
          <span>{fixBusy ? translate(locale, 'fix.generating') : translate(locale, 'action.fix')}</span>
        </button>
      </div>

      {sourceOpenState.status === 'success' && (
        <div className="source-open-status success" role="status" aria-live="polite">
          <strong>{translate(locale, 'source.opened')}</strong>
          <code title={sourceOpenState.displayFile}>
            {sourceOpenState.displayFile}:{sourceOpenState.line}{sourceOpenState.column ? `:${sourceOpenState.column}` : ''}
          </code>
        </div>
      )}
      {sourceOpenState.status === 'failure' && (
        <div className="source-open-status failure" role="status" aria-live="polite">
          <strong>{sourceOpenFailureMessage(locale, sourceOpenState.reason)}</strong>
        </div>
      )}

      {measureState.status === 'success' && (
        <div className="measure-status success" role="status" aria-live="polite">
          <strong>{translate(locale, 'measure.success')} {measureState.width} × {measureState.height} CSS px</strong>
          <span>{translate(locale, 'measure.position')} x {measureState.x} · y {measureState.y}</span>
        </div>
      )}
      {measureState.status === 'failure' && (
        <div className="measure-status failure" role="status" aria-live="polite">
          <strong>{translate(locale, 'measure.failed')}</strong>
        </div>
      )}

      {captureState.status === 'success' && (
        <div className="capture-status success" role="status" aria-live="polite">
          <strong>{translate(locale, 'capture.success')}</strong>
          <span>{captureState.pixelWidth}×{captureState.pixelHeight} · {captureState.backend}</span>
          <code title={captureState.evidenceId}>{translate(locale, 'capture.evidence')} {captureState.evidenceId.slice(0, 12)}</code>
        </div>
      )}
      {captureState.status === 'failure' && (
        <div className="capture-status failure" role="status" aria-live="polite">
          <strong>
            {translate(
              locale,
              captureState.reason === 'unavailable' ? 'capture.unavailable' : 'capture.failed',
            )}
          </strong>
        </div>
      )}

      {current && !live.observer.length && <AttachNotice locale={locale} onOpenNative={onOpenNative} />}
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

function AdvancedPanel({ current, live, locale, onOpenNative }: { current?: Session; live: LiveSessionState; locale: SupportedLocale; onOpenNative: () => void }) {
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
      {!live.observer.length && <AttachNotice locale={locale} onOpenNative={onOpenNative} />}
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
            onChange={(event) => {
              const rememberChromePositions = event.target.checked;
              onPreferencesChange({
                rememberChromePositions,
                ...(rememberChromePositions
                  ? {}
                  : { targetBarPosition: null, toolRailPosition: null }),
              });
            }}
          />
        </label>
        <button className="settings-reset" onClick={onResetWorkspace}>
          {translate(locale, 'action.resetWorkspace')}
        </button>
      </section>
    </div>
  );
}

function ResponsivePanel({ current, locale }: { current?: Session; locale: SupportedLocale }) {
  const presets = [
    [translate(locale, 'responsive.mobileSmall'), '320', '568'],
    [translate(locale, 'responsive.mobile'), '390', '844'],
    [translate(locale, 'responsive.tablet'), '768', '1024'],
    [translate(locale, 'responsive.desktop'), '1440', '900'],
  ];
  const unavailableReason = current
    ? translate(locale, 'responsive.unavailable')
    : translate(locale, 'empty.noTarget');
  return <div>
    <div className="responsive-summary"><span>{translate(locale, 'responsive.viewports')}</span><strong>{current ? current.project.display_name : translate(locale, 'empty.noTarget')}</strong></div>
    <div className="viewport-list">{presets.map(([name, width, height]) => <button key={name} disabled aria-disabled="true" title={unavailableReason}><span className="viewport-icon"/><div><strong>{name}</strong><span>{width} × {height}</span></div><kbd>{width}</kbd></button>)}</div>
    <div className="panel-note">{translate(locale, 'responsive.note')}</div>
  </div>;
}
function ConsolePanel({ live, locale, onOpenNative }: { live: LiveSessionState; locale: SupportedLocale; onOpenNative: () => void }) {
  const events = live.observer.filter((event) => event.kind === 'console' || event.kind === 'runtime_error').slice(-80);
  const eventLabel = translate(locale, events.length === 1 ? 'console.eventOne' : 'console.eventMany');
  return <div className="stream-panel">
    <div className="stream-toolbar"><span className="filter-chip active">{translate(locale, 'console.live')}</span><span className="stream-status"><i className={events.length ? '' : 'muted'} /> {events.length} {eventLabel}</span></div>
    {events.length ? <div className="evidence-stream">{events.map((event) => <ConsoleRow key={`${event.seq}-${event.captured_at}`} event={event} />)}</div> : <EmptyEvidence icon={<ConsoleIcon />} title={translate(locale, 'console.emptyTitle')} text={translate(locale, 'console.emptyText')} action={translate(locale, 'action.openPreview')} onAction={onOpenNative} />}
  </div>;
}

function NetworkPanel({ current, live, locale, onOpenNative }: { current?: Session; live: LiveSessionState; locale: SupportedLocale; onOpenNative: () => void }) {
  const events = live.observer.filter((event) => event.kind === 'network').slice(-100);
  const failures = events.filter((event) => Number(event.payload.status ?? 0) >= 400 || event.payload.ok === false).length;
  return <div className="stream-panel">
    <div className="network-summary">
      <div><span>{translate(locale, 'network.target')}</span><strong>{current ? `:${current.endpoint.port}` : '—'}</strong></div>
      <div><span>{translate(locale, 'network.requests')}</span><strong>{events.length}</strong></div>
      <div><span>{translate(locale, 'network.failures')}</span><strong>{failures}</strong></div>
    </div>
    {events.length ? <div className="evidence-stream network-stream">{events.map((event) => <NetworkRow key={`${event.seq}-${event.captured_at}`} event={event} />)}</div> : <EmptyEvidence icon={<NetworkIcon />} title={translate(locale, 'network.emptyTitle')} text={translate(locale, 'network.emptyText')} action={translate(locale, 'action.openPreview')} onAction={onOpenNative} />}
  </div>;
}

function AiPanel({
  current,
  locale,
  selectedReference,
  providerCapability,
  askAiState,
  onAskAi,
  onRefreshProvider,
  fixCapability,
  fixState,
  verifyState,
  onVerifyChange,
  onBeginFix,
  onPrepareFix,
  onApplyFix,
  onDiscardFix,
  onRefreshFixCapability,
}: {
  current?: Session;
  locale: SupportedLocale;
  selectedReference?: string;
  providerCapability: AiProviderCapability;
  askAiState: HumanAskAiState;
  onAskAi: (question: string) => void;
  onRefreshProvider: () => void;
  fixCapability: AiFixCapability;
  fixState: HumanFixState;
  verifyState: HumanVerifyState;
  onVerifyChange: () => void;
  onBeginFix: () => void;
  onPrepareFix: (instruction: string) => void;
  onApplyFix: () => void;
  onDiscardFix: () => void;
  onRefreshFixCapability: () => void;
}) {
  const [question, setQuestion] = useState(() => translate(locale, 'ai.defaultQuestion'));
  const [fixInstruction, setFixInstruction] = useState(() => translate(locale, 'fix.defaultInstruction'));
  const busy = askAiState.status === 'asking';
  const fixBusy = fixState.status === 'proposing' || fixState.status === 'applying';
  const verifyRetryable = verifyCanRetry(verifyState);
  const verifyScope = verifyState.status === 'ready'
    || verifyState.status === 'verifying'
    || verifyState.status === 'success'
    ? verifyState.scope
    : verifyState.status === 'failure' && verifyRetryable
      ? verifyState.scope
      : undefined;
  const canAsk = !!current && !!selectedReference && providerCapability.available && !busy;
  const unavailableReason = !current
    ? translate(locale, 'empty.noTarget')
    : !selectedReference
      ? translate(locale, 'inspector.noSelection')
      : translate(locale, 'ai.unavailable');

  const submit = () => {
    if (!canAsk) return;
    onAskAi(question);
  };

  return <div className="ai-panel-content human-ai-panel">
    <div className="ai-panel-heading">
      <div className="ai-mark"><SparkIcon /></div>
      <div>
        <h2>AI</h2>
        <span className={`compact-status ${providerCapability.available ? 'success' : ''}`}>
          {providerCapability.available
            ? `${translate(locale, 'ai.providerConnected')}${providerCapability.label ? ` · ${providerCapability.label}` : ''}`
            : translate(locale, 'ai.unavailable')}
        </span>
      </div>
    </div>

    <div className="ai-selection-summary">
      <span>{translate(locale, 'inspector.currentTarget')}</span>
      <strong>{selectedReference ?? translate(locale, 'inspector.noSelection')}</strong>
      {current && <small>{current.project.display_name}</small>}
    </div>

    <label className="ai-question-label">
      <span>{translate(locale, 'ai.question')}</span>
      <textarea
        value={question}
        onChange={(event) => setQuestion(event.target.value)}
        disabled={!current || busy}
        placeholder={translate(locale, 'ai.enterQuestion')}
        rows={4}
      />
    </label>

    <div className="ai-primary-actions">
      <button
        className="ai-submit-action"
        onClick={submit}
        disabled={!canAsk}
        aria-busy={busy}
        title={canAsk ? translate(locale, 'ai.ask') : unavailableReason}
      >
        <SparkIcon />
        <span>{busy ? translate(locale, 'ai.asking') : translate(locale, 'ai.ask')}</span>
      </button>
      {!providerCapability.available && (
        <button className="ai-retry-action" onClick={onRefreshProvider}>
          {translate(locale, 'action.retry')}
        </button>
      )}
    </div>

    <div className="suggestion-grid">
      <button
        onClick={() => onAskAi(translate(locale, 'ai.defaultQuestion'))}
        disabled={!canAsk}
        aria-disabled={!canAsk}
        title={canAsk ? translate(locale, 'ai.askSelection') : unavailableReason}
      >
        {translate(locale, 'ai.askSelection')}
      </button>
      <button disabled aria-disabled="true" title={translate(locale, 'ai.notImplementedYet')}>
        {translate(locale, 'ai.explainIssue')}
      </button>
      <button
        onClick={onBeginFix}
        disabled={!current || !selectedReference || !fixCapability.available || fixBusy}
        aria-disabled={!current || !selectedReference || !fixCapability.available || fixBusy}
        title={
          fixCapability.available
            ? translate(locale, 'ai.fixSelection')
            : translate(locale, 'fix.unavailable')
        }
      >
        {translate(locale, 'ai.fixSelection')}
      </button>
      <button
        onClick={onVerifyChange}
        disabled={verifyState.status !== 'ready' && !verifyRetryable}
        aria-disabled={verifyState.status !== 'ready' && !verifyRetryable}
        aria-busy={verifyState.status === 'verifying'}
        title={
          verifyRetryable
            ? translate(locale, 'action.retry')
            : verifyState.status === 'ready'
              ? translate(locale, 'verify.action')
              : verifyState.status === 'verifying'
                ? translate(locale, 'verify.inProgress')
                : translate(locale, 'verify.ready')
        }
      >
        {verifyRetryable
          ? translate(locale, 'action.retry')
          : verifyState.status === 'verifying'
            ? translate(locale, 'verify.inProgress')
            : translate(locale, 'ai.verifyChange')}
      </button>
    </div>

    {askAiState.status === 'success' && (
      <section className="ai-answer" role="status" aria-live="polite">
        <div className="ai-answer-header">
          <strong>{translate(locale, 'ai.answer')}</strong>
          <span>{askAiState.providerLabel}</span>
        </div>
        <pre className="ai-answer-text">{askAiState.answer}</pre>
        <small>{translate(locale, 'ai.advisory')}</small>
      </section>
    )}

    {askAiState.status === 'failure' && (
      <div className="ai-status failure" role="status" aria-live="polite">
        <strong>{askAiFailureMessage(locale, askAiState.reason)}</strong>
      </div>
    )}

    <section className="fix-review" aria-label={translate(locale, 'fix.review')}>
      <div className="fix-review-heading">
        <div>
          <span>{translate(locale, 'fix.title')}</span>
          <strong>{translate(locale, 'fix.review')}</strong>
        </div>
        <span className={`compact-status ${fixCapability.available ? 'success' : ''}`}>
          {fixCapability.available
            ? `${translate(locale, 'fix.available')}${fixCapability.providerLabel ? ` · ${fixCapability.providerLabel}` : ''}`
            : translate(locale, 'fix.unavailable')}
        </span>
      </div>

      {!fixCapability.available && (
        <div className="fix-unavailable">
          <p>{translate(locale, 'fix.unavailableHint')}</p>
          <button className="fix-refresh-action" onClick={onRefreshFixCapability}>
            {translate(locale, 'action.retry')}
          </button>
        </div>
      )}

      {fixCapability.available && fixState.status === 'idle' && (
        <button className="fix-start-action" onClick={onBeginFix} disabled={!current || !selectedReference}>
          {translate(locale, 'fix.startReview')}
        </button>
      )}

      {fixCapability.available && (fixState.status === 'disclosure' || fixState.status === 'proposing') && (
        <div className="fix-disclosure">
          <p>{translate(locale, 'fix.disclosure')}</p>
          <p className="fix-no-write">{translate(locale, 'fix.noWriteBeforeApply')}</p>
          <label className="fix-instruction-label">
            <span>{translate(locale, 'fix.instruction')}</span>
            <textarea
              value={fixInstruction}
              onChange={(event) => setFixInstruction(event.target.value)}
              disabled={fixState.status === 'proposing'}
              rows={4}
              placeholder={translate(locale, 'fix.enterInstruction')}
            />
          </label>
          <button
            className="fix-generate-action"
            onClick={() => onPrepareFix(fixInstruction)}
            disabled={fixState.status === 'proposing'}
            aria-busy={fixState.status === 'proposing'}
          >
            {fixState.status === 'proposing'
              ? translate(locale, 'fix.generating')
              : translate(locale, 'fix.generate')}
          </button>
        </div>
      )}

      {fixState.status === 'proposal' && (
        <div className="fix-proposal">
          <div className="fix-proposal-meta">
            <strong>{fixState.summary}</strong>
            <code title={fixState.displayFile}>{fixState.displayFile}</code>
            <span>{fixState.providerLabel}</span>
          </div>
          <pre className="fix-diff" tabIndex={0}>{fixState.diff}</pre>
          <p className="fix-advisory">{translate(locale, 'fix.advisory')}</p>
          <div className="fix-proposal-actions">
            <button className="fix-apply-action" onClick={onApplyFix}>
              {translate(locale, 'fix.apply')}
            </button>
            <button className="fix-discard-action" onClick={onDiscardFix}>
              {translate(locale, 'fix.discard')}
            </button>
          </div>
        </div>
      )}

      {fixState.status === 'applying' && (
        <div className="fix-status busy" role="status" aria-live="polite">
          <strong>{translate(locale, 'fix.applying')}</strong>
          <span>{fixState.displayFile}</span>
        </div>
      )}

      {fixState.status === 'success' && (
        <div className="fix-status success" role="status" aria-live="polite">
          <strong>{translate(locale, 'fix.applied')}</strong>
          <span>{fixState.displayFile}</span>
          <button className="fix-again-action" onClick={onBeginFix}>
            {translate(locale, 'fix.startReview')}
          </button>
        </div>
      )}

      {fixState.status === 'failure' && (
        <div className="fix-status failure" role="status" aria-live="polite">
          <strong>{fixFailureMessage(locale, fixState.reason)}</strong>
        </div>
      )}
    </section>

    <section className="verify-review" aria-label={translate(locale, 'verify.title')}>
      <div className="fix-review-heading">
        <div>
          <span>{translate(locale, 'verify.title')}</span>
          <strong>{translate(locale, 'verify.readOnlyDisclosure')}</strong>
        </div>
        {verifyScope && (
          <span className="compact-status success">
            {verifyScope === 'semantic_visual'
              ? translate(locale, 'verify.semanticVisual')
              : translate(locale, 'verify.semanticOnly')}
          </span>
        )}
      </div>

      {verifyState.status === 'idle' && (
        <p className="fix-unavailable">{translate(locale, 'verify.ready')}</p>
      )}

      {verifyState.status === 'ready' && (
        <div className="verify-ready">
          <code title={verifyState.displayFile}>{verifyState.displayFile}</code>
          <button className="fix-start-action" onClick={onVerifyChange}>
            {translate(locale, 'verify.action')}
          </button>
        </div>
      )}

      {verifyState.status === 'verifying' && (
        <div className="fix-status busy" role="status" aria-live="polite">
          <strong>{translate(locale, 'verify.inProgress')}</strong>
          <span>{verifyState.displayFile}</span>
        </div>
      )}

      {verifyState.status === 'success' && (
        <div className={`verify-result ${verifyState.result}`} role="status" aria-live="polite">
          <strong>
            {verifyState.result === 'change_observed'
              ? translate(locale, 'verify.changeObserved')
              : verifyState.result === 'no_observable_change'
                ? translate(locale, 'verify.noObservableChange')
                : verifyState.result === 'regression_signal'
                  ? translate(locale, 'verify.regressionSignal')
                  : translate(locale, 'verify.inconclusive')}
          </strong>
          <span>{verifyState.displayFile}</span>
          <small>{translate(locale, 'verify.objectiveFacts')}</small>
          {verifyState.semanticChanges.length > 0 && (
            <ul>{verifyState.semanticChanges.map((item: string) => <li key={item}>{item}</li>)}</ul>
          )}
          {verifyState.regressionSignals.length > 0 && (
            <ul>{verifyState.regressionSignals.map((item: string) => <li key={item}>{item}</li>)}</ul>
          )}
          {verifyState.advisorySummary && (
            <div className="verify-advisory">
              <span>{translate(locale, 'verify.aiAssessment')}</span>
              <p>{verifyState.advisorySummary}</p>
            </div>
          )}
        </div>
      )}

      {verifyState.status === 'failure' && (
        <div className="fix-status failure" role="status" aria-live="polite">
          <strong>
            {verifyState.reason === 'expired'
              ? translate(locale, 'verify.expired')
              : verifyState.reason === 'source_changed'
                ? translate(locale, 'verify.sourceChanged')
                : verifyState.reason === 'route_changed'
                  ? translate(locale, 'verify.routeChanged')
                  : verifyState.reason === 'target_unavailable'
                    ? translate(locale, 'verify.targetUnavailable')
                    : translate(locale, 'verify.failed')}
          </strong>
          {verifyRetryable && (
            <button className="fix-again-action verify-retry-action" onClick={onVerifyChange}>
              {translate(locale, 'action.retry')}
            </button>
          )}
        </div>
      )}
    </section>

    <p className="ai-privacy-note">{translate(locale, 'ai.privacyNote')}</p>
  </div>;
}

function SessionsPanel({ state, current, locale, onSelect }: { state: DashboardState; current?: Session; locale: SupportedLocale; onSelect: (id: string) => void }) {
  const detectedLabel = state.sessions.length === 1
    ? translate(locale, 'sessions.detectedOne')
    : translate(locale, 'sessions.detectedMany');

  const sessionStatusLabel = (status: Session['status']) => {
    switch (status) {
      case 'active':
        return translate(locale, 'session.status.active');
      case 'disconnected':
        return translate(locale, 'session.status.disconnected');
      case 'hidden':
        return translate(locale, 'session.status.hidden');
      case 'closed':
        return translate(locale, 'session.status.closed');
    }
  };

  return <div className="sessions-panel"><div className="session-overview"><strong>{state.sessions.length}</strong><span>{detectedLabel}</span></div><div className="session-cards">
    {state.sessions.map((session) => <button key={session.id} className={current?.id === session.id ? 'selected' : ''} onClick={() => onSelect(session.id)}><span className={`health-dot ${session.status === 'disconnected' ? 'danger' : session.status === 'hidden' ? 'warn' : ''}`} /><div><strong>{session.project.display_name}</strong><span>{session.classification.framework ?? translate(locale, 'session.framework.web')} · :{session.endpoint.port}</span></div><span className="session-state">{sessionStatusLabel(session.status)}</span></button>)}
    {!state.sessions.length && <PanelEmpty title={translate(locale, 'empty.noTarget')} text={translate(locale, 'empty.runDevServer')} />}
  </div></div>;
}

function CommandPanel({
  state,
  current,
  url,
  locale,
  preferences,
  selectedReference,
  providerCapability,
  fixCapability,
  verifyState,
  onCommand,
}: {
  state: DashboardState;
  current?: Session;
  url?: string;
  locale: SupportedLocale;
  preferences: LocalViewPreferences;
  selectedReference?: string;
  providerCapability: AiProviderCapability;
  fixCapability: AiFixCapability;
  verifyState: HumanVerifyState;
  onCommand: (command: CommandId) => void;
}) {
  const [query, setQuery] = useState('');
  const verifyRetryable = verifyCanRetry(verifyState);
  const commands = [
    { id: COMMAND_IDS.inspectActivate, icon: <InspectIcon />, title: translate(locale, 'tool.inspect'), detail: current?.project.display_name ?? '', keys: 'I', disabled: !current },
    {
      id: COMMAND_IDS.sourceOpen,
      icon: <SourceIcon />,
      title: translate(locale, 'action.openSource'),
      detail: selectedReference ?? (current ? translate(locale, 'source.selectFirst') : translate(locale, 'source.unavailable')),
      keys: '',
      disabled: !current || !selectedReference,
    },
    { id: COMMAND_IDS.responsiveOpen, icon: <ResponsiveIcon />, title: translate(locale, 'tool.responsive'), detail: '', keys: 'R', disabled: !current },
    { id: COMMAND_IDS.consoleOpen, icon: <ConsoleIcon />, title: translate(locale, 'tool.console'), detail: '', keys: 'C', disabled: !current },
    { id: COMMAND_IDS.networkOpen, icon: <NetworkIcon />, title: translate(locale, 'tool.network'), detail: '', keys: 'N', disabled: !current },
    { id: COMMAND_IDS.aiOpen, icon: <SparkIcon />, title: translate(locale, 'tool.ai'), detail: '', keys: 'A', disabled: !current },
    {
      id: COMMAND_IDS.aiAskSelection,
      icon: <SparkIcon />,
      title: translate(locale, 'ai.askSelection'),
      detail: !current
        ? translate(locale, 'empty.noTarget')
        : !selectedReference
          ? translate(locale, 'inspector.noSelection')
          : !providerCapability.available
            ? translate(locale, 'ai.unavailable')
            : selectedReference,
      keys: '',
      disabled: !current || !selectedReference || !providerCapability.available,
    },
    {
      id: COMMAND_IDS.aiFixSelection,
      icon: <ActivityIcon />,
      title: translate(locale, 'ai.fixSelection'),
      detail: !current
        ? translate(locale, 'empty.noTarget')
        : !selectedReference
          ? translate(locale, 'inspector.noSelection')
          : !fixCapability.available
            ? translate(locale, 'fix.unavailable')
            : translate(locale, 'fix.review'),
      keys: '',
      disabled: !current || !selectedReference || !fixCapability.available,
    },
    {
      id: COMMAND_IDS.aiVerifyChange,
      icon: <ActivityIcon />,
      title: translate(locale, 'ai.verifyChange'),
      detail: verifyState.status === 'ready'
        ? verifyState.displayFile
        : verifyRetryable
          ? translate(locale, 'action.retry')
          : translate(locale, 'verify.ready'),
      keys: '',
      disabled: verifyState.status !== 'ready' && !verifyRetryable,
    },
    { id: COMMAND_IDS.previewOpenNative, icon: <ExternalIcon />, title: translate(locale, 'action.openPreview'), detail: url ?? '', keys: '↵', disabled: !current },
    { id: COMMAND_IDS.settingsOpen, icon: <SettingsIcon />, title: translate(locale, 'tool.settings'), detail: '', keys: '⌘,', disabled: false },
    { id: COMMAND_IDS.advancedOpen, icon: <MoreIcon />, title: translate(locale, 'tool.advanced'), detail: '', keys: 'M', disabled: !current },
    {
      id: COMMAND_IDS.workspaceToggleTargetBar,
      icon: <InspectIcon />,
      title: preferences.showTargetBar ? translate(locale, 'action.hideTargetBar') : translate(locale, 'action.showTargetBar'),
      detail: '',
      keys: '⇧⌃T',
      disabled: false,
    },
    {
      id: COMMAND_IDS.workspaceToggleToolRail,
      icon: <CommandIcon />,
      title: preferences.showToolRail ? translate(locale, 'action.hideToolRail') : translate(locale, 'action.showToolRail'),
      detail: '',
      keys: '',
      disabled: false,
    },
    {
      id: COMMAND_IDS.workspaceResetLayout,
      icon: <CommandIcon />,
      title: translate(locale, 'action.resetWorkspace'),
      detail: '',
      keys: '',
      disabled: false,
    },
    {
      id: COMMAND_IDS.sessionPauseDiscovery,
      icon: state.health.paused ? <PlayIcon /> : <PauseIcon />,
      title: state.health.paused ? translate(locale, 'action.resumeDiscovery') : translate(locale, 'action.pauseDiscovery'),
      detail: '',
      keys: 'P',
      disabled: false,
    },
  ];
  const normalizedQuery = query.trim().toLocaleLowerCase();
  const visibleCommands = commands.filter((command) => {
    if (!normalizedQuery) return true;
    return `${command.title} ${command.detail}`.toLocaleLowerCase().includes(normalizedQuery);
  });
  return <div className="command-content">
    <div className="command-search"><SearchIcon /><input autoFocus value={query} onChange={(event) => setQuery(event.target.value)} placeholder={translate(locale, 'command.searchPlaceholder')} aria-label={translate(locale, 'command.searchAria')} /><kbd>ESC</kbd></div>
    <div className="command-list">{visibleCommands.map((command) => <button key={command.id} onClick={() => onCommand(command.id)} disabled={command.disabled}><span className="command-icon">{command.icon}</span><div><strong>{command.title}</strong>{command.detail && <span>{command.detail}</span>}</div>{command.keys && <kbd>{command.keys}</kbd>}</button>)}</div>
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

function AttachNotice({ locale, onOpenNative }: { locale: SupportedLocale; onOpenNative: () => void }) {
  return <button className="attach-notice" onClick={onOpenNative}><WarningIcon /><div><strong>{translate(locale, 'observer.connectTitle')}</strong><span>{translate(locale, 'observer.connectText')}</span></div><ExternalIcon /></button>;
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
    sessions: translate(locale, 'sessions.title'),
    command: translate(locale, 'tool.command'),
  }[tool];
}
function panelEyebrow(tool: ToolId, locale: SupportedLocale) {
  return {
    inspect: translate(locale, 'panel.tools'),
    responsive: translate(locale, 'panel.tools'),
    console: translate(locale, 'panel.tools'),
    network: translate(locale, 'panel.tools'),
    ai: translate(locale, 'panel.tools'),
    advanced: translate(locale, 'panel.diagnostics'),
    settings: translate(locale, 'panel.preferences'),
    sessions: translate(locale, 'panel.sessions'),
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
