import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  type CSSProperties,
  type KeyboardEvent as ReactKeyboardEvent,
  type PointerEvent as ReactPointerEvent,
  type ReactNode,
  type RefObject,
} from 'react';
import { api, type AiFixCapability, type AiProviderCapability, type ResponsivePresetId } from '../api';
import { COMMAND_IDS, type CommandId } from '../commands';
import { applyDocumentLocale, translate } from '../i18n';
import {
  clampChromePoint,
  loadPreferences,
  resetWorkspace,
  updatePreferences as persistPreferences,
  type ChromePoint,
  type LocalViewPreferences,
} from '../preferences';
import type { ActionCorrelationReceipt, DashboardState, LiveSessionState, Session } from '../types';
import { WorkspaceSurface } from './WorkspaceSurface';
import {
  CommandRailButton,
  FloatingPanel,
  RailButton,
  type HumanCaptureState,
  type HumanResponsiveState,
  type HumanAskAiState,
  type HumanFixState,
  type HumanVerifyState,
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
const STABLE_ELEMENT_REFERENCE = /^@e[0-9a-f]+$/i;

function isStableElementReference(reference: unknown): reference is string {
  return typeof reference === 'string'
    && reference.length <= 64
    && STABLE_ELEMENT_REFERENCE.test(reference);
}

function createPointSelectToken(): string {
  if (typeof crypto.randomUUID === 'function') return `point-${crypto.randomUUID()}`;
  const entropy = new Uint32Array(4);
  crypto.getRandomValues(entropy);
  return `point-${Array.from(entropy, (value) => value.toString(16).padStart(8, '0')).join('')}`;
}

const unavailableAiProvider: AiProviderCapability = {
  available: false,
  label: null,
  reason: 'not_configured',
};

const unavailableFixCapability: AiFixCapability = {
  available: false,
  providerLabel: null,
  reason: 'not_enabled',
};

type ChromePositionKey = 'targetBarPosition' | 'toolRailPosition';

interface ChromeMover {
  ref: RefObject<HTMLElement | null>;
  style?: CSSProperties;
  onPointerDown: (event: ReactPointerEvent<HTMLButtonElement>) => void;
  onKeyDown: (event: ReactKeyboardEvent<HTMLButtonElement>) => void;
}

function sameChromePoint(left: ChromePoint | null, right: ChromePoint | null): boolean {
  if (left === right) return true;
  if (!left || !right) return false;
  return Math.abs(left.x - right.x) < 0.5 && Math.abs(left.y - right.y) < 0.5;
}

function useMovableChrome(
  position: ChromePoint | null,
  onPreview: (point: ChromePoint) => void,
  onCommit: (point: ChromePoint | null) => void,
): ChromeMover {
  const ref = useRef<HTMLElement | null>(null);

  const clampForNode = useCallback((point: ChromePoint): ChromePoint => {
    const node = ref.current;
    if (!node) return point;
    const rect = node.getBoundingClientRect();
    return clampChromePoint(
      point,
      { width: rect.width, height: rect.height },
      { width: window.innerWidth, height: window.innerHeight },
    );
  }, []);

  useEffect(() => {
    if (!position) return;
    const reclamp = () => {
      const next = clampForNode(position);
      if (!sameChromePoint(position, next)) onCommit(next);
    };
    reclamp();
    window.addEventListener('resize', reclamp);
    return () => window.removeEventListener('resize', reclamp);
  }, [clampForNode, onCommit, position]);

  const onPointerDown = useCallback((event: ReactPointerEvent<HTMLButtonElement>) => {
    if (event.button !== 0) return;
    const node = ref.current;
    if (!node) return;

    event.preventDefault();
    const handle = event.currentTarget;
    const pointerId = event.pointerId;
    const startPointer = { x: event.clientX, y: event.clientY };
    const startRect = node.getBoundingClientRect();
    let latest = clampForNode({ x: startRect.left, y: startRect.top });
    let finished = false;

    handle.setPointerCapture(pointerId);

    const move = (nextEvent: PointerEvent) => {
      if (nextEvent.pointerId !== pointerId) return;
      latest = clampForNode({
        x: startRect.left + nextEvent.clientX - startPointer.x,
        y: startRect.top + nextEvent.clientY - startPointer.y,
      });
      onPreview(latest);
    };

    const finish = (nextEvent: PointerEvent) => {
      if (finished || nextEvent.pointerId !== pointerId) return;
      finished = true;
      window.removeEventListener('pointermove', move);
      window.removeEventListener('pointerup', finish);
      window.removeEventListener('pointercancel', finish);
      if (handle.hasPointerCapture(pointerId)) handle.releasePointerCapture(pointerId);
      onCommit(latest);
    };

    window.addEventListener('pointermove', move);
    window.addEventListener('pointerup', finish);
    window.addEventListener('pointercancel', finish);
  }, [clampForNode, onCommit, onPreview]);

  const onKeyDown = useCallback((event: ReactKeyboardEvent<HTMLButtonElement>) => {
    if (event.key === 'Home') {
      event.preventDefault();
      onCommit(null);
      return;
    }

    const directions: Record<string, [number, number]> = {
      ArrowLeft: [-1, 0],
      ArrowRight: [1, 0],
      ArrowUp: [0, -1],
      ArrowDown: [0, 1],
    };
    const direction = directions[event.key];
    if (!direction) return;

    const node = ref.current;
    if (!node) return;
    event.preventDefault();
    const rect = node.getBoundingClientRect();
    const step = event.shiftKey ? 1 : 12;
    onCommit(clampForNode({
      x: rect.left + direction[0] * step,
      y: rect.top + direction[1] * step,
    }));
  }, [clampForNode, onCommit]);

  const style = useMemo<CSSProperties | undefined>(
    () => position
      ? {
          left: position.x,
          top: position.y,
          right: 'auto',
          transform: 'none',
        }
      : undefined,
    [position],
  );

  return { ref, style, onPointerDown, onKeyDown };
}

function classifyFixFailure(
  cause: unknown,
): Extract<HumanFixState, { status: 'failure' }>['reason'] {
  const detail = String(cause).toLowerCase();

  if (detail.includes('instruction exceeds the safety bound')) return 'instruction_too_long';
  if (detail.includes('instruction is empty') || detail.includes('instruction is invalid')) {
    return 'invalid_instruction';
  }
  if (detail.includes('sensitive source')) return 'sensitive_source';
  if (detail.includes('source type is unsupported')) return 'unsupported_source';
  if (detail.includes('proposal expired')) return 'proposal_expired';
  if (
    detail.includes('proposal is unavailable')
    || detail.includes('proposal is no longer pending')
    || detail.includes('proposal is not applying')
  ) {
    return 'proposal_invalid';
  }
  if (
    detail.includes('source changed')
    || detail.includes('route changed')
    || detail.includes('source mapping changed')
  ) {
    return 'source_changed';
  }
  if (
    detail.includes('source mapping is unavailable')
    || detail.includes('source is unavailable')
    || detail.includes('project root is unavailable')
    || detail.includes('source outside project')
    || detail.includes('source symlink')
  ) {
    return 'source_unavailable';
  }
  if (
    detail.includes('provider unavailable')
    || detail.includes('provider is not configured')
    || detail.includes('provider configuration is unsupported')
    || detail.includes('provider bridge must be loopback')
  ) {
    return 'provider_unavailable';
  }
  if (
    detail.includes('write transaction')
    || detail.includes('replace the source file')
    || detail.includes('post-write verification')
    || detail.includes('rollback')
    || detail.includes('temporary write')
    || detail.includes('preserve file permissions')
  ) {
    return 'apply_failed';
  }
  return 'failed';
}

function classifyAskAiFailure(
  cause: unknown,
): 'provider_unavailable' | 'context_unavailable' | 'invalid_question' | 'question_too_long' | 'failed' {
  const detail = String(cause).toLowerCase();

  if (detail.includes('trusted ai question exceeds the safety bound')) {
    return 'question_too_long';
  }
  const invalidQuestionPhrases = [
    'trusted ai question is empty',
    'trusted ai question is invalid',
  ];
  if (invalidQuestionPhrases.some((phrase) => detail.includes(phrase))) {
    return 'invalid_question';
  }

  const providerUnavailablePhrases = [
    'trusted ai provider unavailable',
    'trusted ai provider is not configured',
    'trusted ai provider configuration is unsupported',
    'trusted ai provider bridge must be loopback',
  ];
  if (providerUnavailablePhrases.some((phrase) => detail.includes(phrase))) {
    return 'provider_unavailable';
  }

  const contextUnavailablePhrases = [
    'trusted ai runtime unavailable',
    'trusted ai session is unavailable',
    'trusted ai context is unavailable',
    'trusted ai selection is no longer available',
    'trusted ai selection is ambiguous',
    'trusted ai selection is unavailable',
    'trusted ai route changed before context resolution',
    'trusted ai route changed while context was being prepared',
    'managed surface',
    'surface is unavailable',
  ];
  return contextUnavailablePhrases.some((phrase) => detail.includes(phrase))
    ? 'context_unavailable'
    : 'failed';
}

function classifyVerifyFailure(
  cause: unknown,
): Extract<HumanVerifyState, { status: 'failure' }>['reason'] {
  const detail = String(cause).toLowerCase();
  if (
    detail.includes('verification expired')
    || detail.includes('record expired')
    || detail.includes('verification is unavailable')
    || detail.includes('record is unavailable')
    || detail.includes('record is not pending')
  ) {
    return 'expired';
  }
  if (detail.includes('settle failed')) {
    return 'settle_failed';
  }
  if (detail.includes('source changed') || detail.includes('source mapping changed')) {
    return 'source_changed';
  }
  if (detail.includes('route changed')) {
    return 'route_changed';
  }
  if (
    detail.includes('target is unavailable')
    || detail.includes('selection is no longer available')
    || detail.includes('selection is ambiguous')
    || detail.includes('element reference')
  ) {
    return 'target_unavailable';
  }
  return 'failed';
}

function classifyResponsiveFailure(
  cause: unknown,
): Extract<HumanResponsiveState, { status: 'failure' }>['reason'] {
  const detail = String(cause).toLowerCase();
  if (
    detail.includes('responsive_preview_unavailable')
    || detail.includes('responsive_preview_owner_mismatch')
    || detail.includes('responsive_preview_maximized')
    || detail.includes('responsive_preview_fullscreen')
  ) {
    return 'preview_unavailable';
  }
  if (detail.includes('responsive_invalid_presets')) return 'invalid_presets';
  if (detail.includes('responsive_route_drift')) return 'route_changed';
  if (detail.includes('responsive_restore_failed')) return 'restore_failed';
  return 'failed';
}

function classifySourceOpenFailure(cause: unknown): 'launcher_unavailable' | 'unavailable' | 'failed' {
  const detail = String(cause).toLowerCase();

  if (detail.includes('trusted source launcher unavailable')) {
    return 'launcher_unavailable';
  }

  const trustedUnavailablePhrases = [
    'trusted source mapping is unavailable',
    'trusted source file is unavailable',
    'trusted source project root is unavailable',
    'trusted source selection is no longer available',
    'trusted source selection is ambiguous',
    'trusted source outside project',
    'trusted source path traversal is not allowed',
    'trusted source path uses a non-native separator',
    'trusted source path is invalid',
    'trusted source path must be project relative',
    'trusted source path prefix is not allowed',
    'trusted source line is unavailable',
    'trusted source file exceeds verification bound',
    'trusted source symlink escape',
  ];

  return trustedUnavailablePhrases.some((phrase) => detail.includes(phrase))
    ? 'unavailable'
    : 'failed';
}

export default function LocalViewShell() {
  const [state, setState] = useState<DashboardState>(fallback);
  const [selected, setSelected] = useState<string>();
  const [activeTool, setActiveTool] = useState<ToolId>();
  const [error, setError] = useState<string>();
  const [live, setLive] = useState<LiveSessionState>(emptyLive);
  const [actionCorrelation, setActionCorrelation] = useState<ActionCorrelationReceipt>();
  const [immersive, setImmersive] = useState(false);
  const [preferences, setPreferences] = useState<LocalViewPreferences>(() => loadPreferences());
  const [targetBarPosition, setTargetBarPosition] = useState<ChromePoint | null>(
    () => preferences.rememberChromePositions ? preferences.targetBarPosition : null,
  );
  const [toolRailPosition, setToolRailPosition] = useState<ChromePoint | null>(
    () => preferences.rememberChromePositions ? preferences.toolRailPosition : null,
  );
  const [responsiveState, setResponsiveState] = useState<HumanResponsiveState>({ status: 'idle' });
  const [captureState, setCaptureState] = useState<HumanCaptureState>({ status: 'idle' });
  const [measureState, setMeasureState] = useState<HumanMeasureState>({ status: 'idle' });
  const [sourceOpenState, setSourceOpenState] = useState<HumanSourceOpenState>({ status: 'idle' });
  const [aiProviderCapability, setAiProviderCapability] = useState<AiProviderCapability>(unavailableAiProvider);
  const [askAiState, setAskAiState] = useState<HumanAskAiState>({ status: 'idle' });
  const [fixCapability, setFixCapability] = useState<AiFixCapability>(unavailableFixCapability);
  const [fixState, setFixState] = useState<HumanFixState>({ status: 'idle' });
  const [verifyState, setVerifyState] = useState<HumanVerifyState>({ status: 'idle' });
  const [pointSelectedReference, setPointSelectedReference] = useState<string>();
  const [pointSelectActive, setPointSelectActive] = useState(false);
  const responsiveInFlight = useRef(false);
  const responsiveGeneration = useRef(0);
  const captureInFlight = useRef(false);
  const captureGeneration = useRef(0);
  const measureInFlight = useRef(false);
  const measureGeneration = useRef(0);
  const sourceOpenInFlight = useRef(false);
  const sourceOpenGeneration = useRef(0);
  const askAiInFlight = useRef(false);
  const askAiGeneration = useRef(0);
  const fixProposalInFlight = useRef(false);
  const fixApplyInFlight = useRef(false);
  const fixGeneration = useRef(0);
  const verifyInFlight = useRef(false);
  const verifyGeneration = useRef(0);
  const correlationGeneration = useRef(0);
  const fixProposalIdRef = useRef<string | undefined>(undefined);
  const selectedReferenceRef = useRef<string | undefined>(undefined);
  const currentSessionIdRef = useRef<string | undefined>(undefined);
  const pointSelectGeneration = useRef(0);
  const pointSelectTokenRef = useRef<string | undefined>(undefined);
  const pointSelectionRouteSequenceRef = useRef(0);
  const latestRouteSequenceRef = useRef(0);

  const patchPreferences = useCallback((patch: Partial<LocalViewPreferences>) => {
    setPreferences((current) => persistPreferences(current, patch));
  }, []);

  const previewChromePosition = useCallback((key: ChromePositionKey, point: ChromePoint) => {
    if (key === 'targetBarPosition') {
      setTargetBarPosition(point);
    } else {
      setToolRailPosition(point);
    }
  }, []);

  const commitChromePosition = useCallback((key: ChromePositionKey, point: ChromePoint | null) => {
    if (key === 'targetBarPosition') {
      setTargetBarPosition(point);
    } else {
      setToolRailPosition(point);
    }
    if (preferences.rememberChromePositions) {
      patchPreferences({ [key]: point } as Partial<LocalViewPreferences>);
    }
  }, [patchPreferences, preferences.rememberChromePositions]);

  const targetBarMover = useMovableChrome(
    targetBarPosition,
    (point) => previewChromePosition('targetBarPosition', point),
    (point) => commitChromePosition('targetBarPosition', point),
  );
  const toolRailMover = useMovableChrome(
    toolRailPosition,
    (point) => previewChromePosition('toolRailPosition', point),
    (point) => commitChromePosition('toolRailPosition', point),
  );

  const resetWorkspacePreferences = useCallback(() => {
    setTargetBarPosition(null);
    setToolRailPosition(null);
    setPreferences((current) => resetWorkspace(current));
  }, []);

  useEffect(() => {
    if (preferences.rememberChromePositions) {
      setTargetBarPosition(preferences.targetBarPosition);
      setToolRailPosition(preferences.toolRailPosition);
    } else {
      setTargetBarPosition(null);
      setToolRailPosition(null);
    }
  }, [
    preferences.rememberChromePositions,
    preferences.targetBarPosition,
    preferences.toolRailPosition,
  ]);

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

  const refreshAiProviderCapability = useCallback(async () => {
    try {
      const capability = await api.aiProviderCapability();
      setAiProviderCapability(capability);
    } catch {
      setAiProviderCapability({
        available: false,
        label: null,
        reason: 'unsupported',
      });
    }
  }, []);

  useEffect(() => {
    void refreshAiProviderCapability();
    const timer = window.setInterval(() => void refreshAiProviderCapability(), 10_000);
    return () => window.clearInterval(timer);
  }, [refreshAiProviderCapability]);

  const refreshFixCapability = useCallback(async () => {
    try {
      const capability = await api.aiFixCapability();
      setFixCapability(capability);
    } catch {
      setFixCapability(unavailableFixCapability);
    }
  }, []);

  useEffect(() => {
    void refreshFixCapability();
    const timer = window.setInterval(() => void refreshFixCapability(), 10_000);
    return () => window.clearInterval(timer);
  }, [refreshFixCapability]);

  const current = useMemo(
    () => state.sessions.find((session) => session.id === selected) ?? state.sessions[0],
    [state.sessions, selected],
  );

  const latestRouteSequence = useMemo(
    () => live.observer.reduce(
      (latest, event) => event.kind === 'route' ? Math.max(latest, Number(event.seq) || 0) : latest,
      0,
    ),
    [live.observer],
  );

  const focusSelectedReference = useMemo(
    () => [...live.observer]
      .reverse()
      .find(
        (event) =>
          event.kind === 'focus'
          && (Number(event.seq) || 0) > latestRouteSequence
          && isStableElementReference(event.reference),
      )
      ?.reference,
    [latestRouteSequence, live.observer],
  );

  const selectedReference = pointSelectedReference ?? focusSelectedReference;

  useEffect(() => {
    selectedReferenceRef.current = selectedReference;
    measureGeneration.current += 1;
    measureInFlight.current = false;
    setMeasureState({ status: 'idle' });
    sourceOpenGeneration.current += 1;
    sourceOpenInFlight.current = false;
    setSourceOpenState({ status: 'idle' });
    askAiGeneration.current += 1;
    askAiInFlight.current = false;
    setAskAiState({ status: 'idle' });
    fixGeneration.current += 1;
    fixProposalInFlight.current = false;
    fixApplyInFlight.current = false;
    const staleProposalId = fixProposalIdRef.current;
    fixProposalIdRef.current = undefined;
    if (staleProposalId) void api.discardFixProposal({ proposalId: staleProposalId });
    setFixState({ status: 'idle' });
    verifyGeneration.current += 1;
    verifyInFlight.current = false;
    setVerifyState({ status: 'idle' });
  }, [selectedReference]);

  useEffect(() => {
    const previousSessionId = currentSessionIdRef.current;
    const stalePointToken = pointSelectTokenRef.current;
    if (previousSessionId && stalePointToken && previousSessionId !== current?.id) {
      void api.cancelPointSelect(previousSessionId, stalePointToken).catch(() => undefined);
    }
    pointSelectGeneration.current += 1;
    pointSelectTokenRef.current = undefined;
    pointSelectionRouteSequenceRef.current = 0;
    setPointSelectActive(false);
    setPointSelectedReference(undefined);
    currentSessionIdRef.current = current?.id;
    responsiveGeneration.current += 1;
    responsiveInFlight.current = false;
    setResponsiveState({ status: 'idle' });
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
    askAiGeneration.current += 1;
    askAiInFlight.current = false;
    setAskAiState({ status: 'idle' });
    fixGeneration.current += 1;
    fixProposalInFlight.current = false;
    fixApplyInFlight.current = false;
    const staleProposalId = fixProposalIdRef.current;
    fixProposalIdRef.current = undefined;
    if (staleProposalId) void api.discardFixProposal({ proposalId: staleProposalId });
    setFixState({ status: 'idle' });
    verifyGeneration.current += 1;
    verifyInFlight.current = false;
    setVerifyState({ status: 'idle' });
    correlationGeneration.current += 1;
    setActionCorrelation(undefined);
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

  useEffect(() => {
    latestRouteSequenceRef.current = latestRouteSequence;
    if (latestRouteSequence <= pointSelectionRouteSequenceRef.current) return;

    setPointSelectedReference(undefined);
    pointSelectionRouteSequenceRef.current = latestRouteSequence;
    const requestToken = pointSelectTokenRef.current;
    const requestSessionId = currentSessionIdRef.current;
    if (requestToken && requestSessionId) {
      pointSelectGeneration.current += 1;
      pointSelectTokenRef.current = undefined;
      setPointSelectActive(false);
      void api.cancelPointSelect(requestSessionId, requestToken).catch(() => undefined);
    }
  }, [latestRouteSequence]);

  useEffect(() => () => {
    const requestToken = pointSelectTokenRef.current;
    const requestSessionId = currentSessionIdRef.current;
    pointSelectGeneration.current += 1;
    if (requestToken && requestSessionId) {
      void api.cancelPointSelect(requestSessionId, requestToken).catch(() => undefined);
    }
  }, []);

  const latestActionId = live.action_results.at(-1)?.action_id;

  useEffect(() => {
    if (activeTool !== 'network' || !current || !latestActionId) return;

    let cancelled = false;
    const generation = ++correlationGeneration.current;
    const requestSessionId = current.id;
    const requestActionId = latestActionId;
    setActionCorrelation(undefined);

    const read = async () => {
      for (let attempt = 0; attempt < 4 && !cancelled; attempt += 1) {
        try {
          const receipt = await api.actionCorrelation(requestSessionId, requestActionId);
          if (
            cancelled
            || generation !== correlationGeneration.current
            || requestSessionId !== currentSessionIdRef.current
          ) {
            return;
          }
          if (receipt && receipt.trace.action_id === requestActionId) {
            setActionCorrelation(receipt);
            return;
          }
        } catch {
          return;
        }
        await new Promise<void>((resolve) => window.setTimeout(resolve, 550));
      }
    };

    void read();
    return () => {
      cancelled = true;
      correlationGeneration.current += 1;
    };
  }, [activeTool, current?.id, latestActionId]);

  const currentUrl = current
    ? `${current.endpoint.scheme}://${current.endpoint.host}:${current.endpoint.port}/`
    : undefined;

  const togglePanel = useCallback((tool: ToolId) => {
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

  const cancelPointSelect = useCallback((closeInspector = false) => {
    const requestToken = pointSelectTokenRef.current;
    const requestSessionId = currentSessionIdRef.current;
    pointSelectGeneration.current += 1;
    pointSelectTokenRef.current = undefined;
    setPointSelectActive(false);
    if (requestToken && requestSessionId) {
      void api.cancelPointSelect(requestSessionId, requestToken).catch(() => undefined);
    }
    if (closeInspector) setActiveTool(undefined);
  }, []);

  const beginPointSelect = useCallback(async () => {
    const session = current;
    if (!session) return;

    const previousToken = pointSelectTokenRef.current;
    const previousSessionId = currentSessionIdRef.current;
    if (previousToken && previousSessionId) {
      void api.cancelPointSelect(previousSessionId, previousToken).catch(() => undefined);
    }

    const generation = ++pointSelectGeneration.current;
    const requestSessionId = session.id;
    const requestToken = createPointSelectToken();
    pointSelectTokenRef.current = requestToken;
    currentSessionIdRef.current = requestSessionId;
    pointSelectionRouteSequenceRef.current = latestRouteSequenceRef.current;
    setPointSelectActive(true);
    setActiveTool('inspect');
    setError(undefined);

    const stillOwnsRequest = () =>
      generation === pointSelectGeneration.current
      && pointSelectTokenRef.current === requestToken
      && currentSessionIdRef.current === requestSessionId;

    try {
      let status;
      try {
        status = await api.beginPointSelect(requestSessionId, requestToken);
      } catch (cause) {
        const detail = String(cause);
        if (
          !detail.includes('point_select_managed_surface_unavailable')
          && !detail.includes('managed surface unavailable')
        ) {
          throw cause;
        }
        await api.openPreview(
          requestSessionId,
          `${session.endpoint.scheme}://${session.endpoint.host}:${session.endpoint.port}/`,
          session.project.display_name,
        );
        if (!stillOwnsRequest()) return;
        status = await api.beginPointSelect(requestSessionId, requestToken);
      }

      while (stillOwnsRequest() && status.state === 'pending') {
        await new Promise<void>((resolve) => window.setTimeout(resolve, 100));
        if (!stillOwnsRequest()) return;
        status = await api.pointSelectStatus(requestSessionId, requestToken);
      }
      if (!stillOwnsRequest()) return;
      if (status.requestToken !== requestToken || status.sessionId !== requestSessionId) return;

      if (status.state === 'selected') {
        if (!isStableElementReference(status.reference)) {
          throw new Error('point_select_invalid_stable_reference');
        }
        pointSelectionRouteSequenceRef.current = latestRouteSequenceRef.current;
        setPointSelectedReference(status.reference);
      } else if (status.state === 'failed') {
        if (
          status.reason === 'route_changed'
          || status.reason === 'generation_changed'
          || status.reason === 'managed_surface_unavailable'
        ) {
          setPointSelectedReference(undefined);
        }
        setError(`Point selection failed: ${status.reason ?? 'failed'}`);
      }

      if (status.state !== 'pending') {
        pointSelectTokenRef.current = undefined;
        setPointSelectActive(false);
      }
    } catch (cause) {
      if (!stillOwnsRequest()) return;
      pointSelectTokenRef.current = undefined;
      setPointSelectActive(false);
      void api.cancelPointSelect(requestSessionId, requestToken).catch(() => undefined);
      setError(String(cause));
    }
  }, [current]);

  const toggleTool = useCallback((tool: ToolId) => {
    if (tool !== 'inspect') {
      togglePanel(tool);
      return;
    }
    if (pointSelectActive) {
      cancelPointSelect(true);
      return;
    }
    setActiveTool('inspect');
    void beginPointSelect();
  }, [beginPointSelect, cancelPointSelect, pointSelectActive, togglePanel]);

  const captureResponsiveSweep = useCallback(async (presets: ResponsivePresetId[]) => {
    const session = current;
    if (!session || responsiveInFlight.current || presets.length === 0) return;

    const requestSessionId = session.id;
    const requestedPresets = [...presets];
    responsiveInFlight.current = true;
    const generation = ++responsiveGeneration.current;
    currentSessionIdRef.current = requestSessionId;
    setResponsiveState({ status: 'running', presets: requestedPresets });

    try {
      const receipt = await api.captureResponsiveSweep({
        sessionId: requestSessionId,
        presets: requestedPresets,
      });
      if (
        generation !== responsiveGeneration.current
        || requestSessionId !== currentSessionIdRef.current
      ) {
        return;
      }
      setResponsiveState({
        status: 'success',
        presets: requestedPresets,
        evidenceId: receipt.evidence_id,
        contactSheetPixelWidth: receipt.contact_sheet_pixel_width,
        contactSheetPixelHeight: receipt.contact_sheet_pixel_height,
        viewports: receipt.viewports.map((viewport) => ({
          preset: viewport.preset,
          cssWidth: viewport.css_width,
          cssHeight: viewport.css_height,
        })),
      });
    } catch (cause) {
      if (
        generation !== responsiveGeneration.current
        || requestSessionId !== currentSessionIdRef.current
      ) {
        return;
      }
      setResponsiveState({
        status: 'failure',
        presets: requestedPresets,
        reason: classifyResponsiveFailure(cause),
      });
    } finally {
      if (generation === responsiveGeneration.current) {
        responsiveInFlight.current = false;
      }
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
      const reason = classifySourceOpenFailure(cause);
      setSourceOpenState({ status: 'failure', reference: sourceOpenReference, reason });
    } finally {
      if (generation === sourceOpenGeneration.current) {
        sourceOpenInFlight.current = false;
      }
    }
  }, [current]);

  const askAiAboutSelection = useCallback(async (question: string) => {
    const session = current;
    const reference = selectedReference;
    if (
      !session
      || !reference
      || !aiProviderCapability.available
      || askAiInFlight.current
    ) {
      return;
    }

    const normalizedQuestion = question.trim();
    const questionBytes = new TextEncoder().encode(normalizedQuestion).length;
    if (!normalizedQuestion || questionBytes > 8 * 1024) {
      setActiveTool('ai');
      setAskAiState({
        status: 'failure',
        reference,
        reason: questionBytes > 8 * 1024 ? 'question_too_long' : 'invalid_question',
      });
      return;
    }

    const requestSessionId = session.id;
    const requestReference = reference;
    const generation = ++askAiGeneration.current;
    askAiInFlight.current = true;
    selectedReferenceRef.current = requestReference;
    currentSessionIdRef.current = requestSessionId;
    setActiveTool('ai');
    setAskAiState({
      status: 'asking',
      reference: requestReference,
      question: normalizedQuestion,
    });

    try {
      const receipt = await api.askAiAboutSelection({
        sessionId: requestSessionId,
        reference: requestReference,
        question: normalizedQuestion,
      });
      if (
        generation !== askAiGeneration.current
        || requestReference !== selectedReferenceRef.current
        || requestSessionId !== currentSessionIdRef.current
      ) {
        return;
      }
      setAskAiState({
        status: 'success',
        reference: requestReference,
        question: normalizedQuestion,
        answer: receipt.answer,
        providerLabel: receipt.providerLabel,
        snapshotVersion: receipt.snapshotVersion,
      });
    } catch (cause) {
      if (
        generation !== askAiGeneration.current
        || requestReference !== selectedReferenceRef.current
        || requestSessionId !== currentSessionIdRef.current
      ) {
        return;
      }
      setAskAiState({
        status: 'failure',
        reference: requestReference,
        reason: classifyAskAiFailure(cause),
      });
    } finally {
      if (generation === askAiGeneration.current) {
        askAiInFlight.current = false;
      }
    }
  }, [aiProviderCapability.available, current, selectedReference]);

  const beginFixReview = useCallback(() => {
    const session = current;
    const reference = selectedReference;
    if (!session || !reference || !fixCapability.available) return;

    const oldProposal = fixProposalIdRef.current;
    fixGeneration.current += 1;
    fixProposalInFlight.current = false;
    fixApplyInFlight.current = false;
    fixProposalIdRef.current = undefined;
    if (oldProposal) void api.discardFixProposal({ proposalId: oldProposal });
    setActiveTool('ai');
    verifyGeneration.current += 1;
    verifyInFlight.current = false;
    setVerifyState({ status: 'idle' });
    setFixState({ status: 'disclosure', reference });
  }, [current, fixCapability.available, selectedReference]);

  const prepareFixProposal = useCallback(async (instruction: string) => {
    const session = current;
    const reference = selectedReference;
    if (
      !session
      || !reference
      || !fixCapability.available
      || fixProposalInFlight.current
      || fixApplyInFlight.current
    ) {
      return;
    }

    const normalized = instruction.trim();
    const bytes = new TextEncoder().encode(normalized).length;
    if (!normalized || bytes > 8 * 1024) {
      setActiveTool('ai');
      setFixState({
        status: 'failure',
        reference,
        reason: bytes > 8 * 1024 ? 'instruction_too_long' : 'invalid_instruction',
      });
      return;
    }

    const requestSessionId = session.id;
    const requestReference = reference;
    const generation = ++fixGeneration.current;
    fixProposalInFlight.current = true;
    selectedReferenceRef.current = requestReference;
    currentSessionIdRef.current = requestSessionId;
    setActiveTool('ai');
    setFixState({ status: 'proposing', reference: requestReference, instruction: normalized });

    try {
      const receipt = await api.prepareFixProposal({
        sessionId: requestSessionId,
        reference: requestReference,
        instruction: normalized,
      });
      if (
        generation !== fixGeneration.current
        || requestReference !== selectedReferenceRef.current
        || requestSessionId !== currentSessionIdRef.current
      ) {
        void api.discardFixProposal({ proposalId: receipt.proposalId });
        return;
      }

      fixProposalIdRef.current = receipt.proposalId;
      setFixState({
        status: 'proposal',
        proposalId: receipt.proposalId,
        reference: requestReference,
        instruction: normalized,
        displayFile: receipt.displayFile,
        summary: receipt.summary,
        diff: receipt.diff,
        providerLabel: receipt.providerLabel,
        expiresAtUnixMs: receipt.expiresAtUnixMs,
      });
    } catch (cause) {
      if (
        generation !== fixGeneration.current
        || requestReference !== selectedReferenceRef.current
        || requestSessionId !== currentSessionIdRef.current
      ) {
        return;
      }
      setFixState({
        status: 'failure',
        reference: requestReference,
        reason: classifyFixFailure(cause),
      });
    } finally {
      if (generation === fixGeneration.current) {
        fixProposalInFlight.current = false;
      }
    }
  }, [current, fixCapability.available, selectedReference]);

  const applyFixProposal = useCallback(async () => {
    if (fixState.status !== 'proposal' || fixApplyInFlight.current) return;

    const proposal = fixState;
    const generation = fixGeneration.current;
    fixApplyInFlight.current = true;
    setFixState({
      status: 'applying',
      proposalId: proposal.proposalId,
      reference: proposal.reference,
      displayFile: proposal.displayFile,
    });

    try {
      const receipt = await api.applyFixProposal({ proposalId: proposal.proposalId });
      if (generation !== fixGeneration.current) return;
      fixProposalIdRef.current = undefined;
      setFixState({
        status: 'success',
        displayFile: receipt.displayFile,
        changedStartLine: receipt.changedStartLine,
        changedEndLine: receipt.changedEndLine,
      });
      setVerifyState({
        status: 'ready',
        verificationId: receipt.verificationId,
        reference: receipt.reference,
        displayFile: receipt.displayFile,
        scope: receipt.verificationScope,
      });
    } catch (cause) {
      if (generation !== fixGeneration.current) return;
      fixProposalIdRef.current = undefined;
      setFixState({
        status: 'failure',
        reference: proposal.reference,
        reason: classifyFixFailure(cause),
      });
    } finally {
      if (generation === fixGeneration.current) {
        fixApplyInFlight.current = false;
      }
    }
  }, [fixState]);

  const discardFixProposal = useCallback(async () => {
    const proposalId = fixProposalIdRef.current;
    const generation = ++fixGeneration.current;
    fixProposalInFlight.current = false;
    fixApplyInFlight.current = false;
    fixProposalIdRef.current = undefined;
    if (proposalId) {
      try {
        await api.discardFixProposal({ proposalId });
      } catch {
        // The backend proposal may already be expired or invalidated.
      }
    }
    if (generation === fixGeneration.current) {
      setFixState({ status: 'idle' });
    }
  }, []);

  const verifyFixChange = useCallback(async () => {
    if (verifyInFlight.current) return;

    const retryableFailure = verifyState.status === 'failure'
      && (verifyState.reason === 'settle_failed' || verifyState.reason === 'failed');
    if (verifyState.status !== 'ready' && verifyState.status !== 'failure') return;
    if (verifyState.status === 'failure' && !retryableFailure) return;

    const verificationId = verifyState.verificationId;
    const reference = verifyState.reference;
    const displayFile = verifyState.displayFile;
    const scope = verifyState.scope;
    const requestSessionId = current?.id;
    if (!verificationId || !reference || !displayFile || !scope || !requestSessionId) return;

    const generation = ++verifyGeneration.current;
    verifyInFlight.current = true;
    currentSessionIdRef.current = requestSessionId;
    setActiveTool('ai');
    setVerifyState({
      status: 'verifying',
      verificationId,
      reference,
      displayFile,
      scope,
    });

    try {
      const receipt = await api.verifyFixChange({ verificationId });
      if (
        generation !== verifyGeneration.current
        || requestSessionId !== currentSessionIdRef.current
        || reference !== selectedReferenceRef.current
      ) {
        return;
      }
      setVerifyState({
        status: 'success',
        verificationId: receipt.verificationId,
        reference: receipt.reference,
        displayFile: receipt.displayFile,
        scope: receipt.scope,
        result: receipt.status,
        semanticChanges: receipt.semanticChanges,
        regressionSignals: receipt.regressionSignals,
        viewportChangedRatio: receipt.viewportChangedRatio,
        targetChangedRatio: receipt.targetChangedRatio,
        providerLabel: receipt.providerLabel,
        advisorySummary: receipt.advisorySummary,
      });
    } catch (cause) {
      if (
        generation !== verifyGeneration.current
        || requestSessionId !== currentSessionIdRef.current
        || reference !== selectedReferenceRef.current
      ) {
        return;
      }
      setVerifyState({
        status: 'failure',
        verificationId,
        reference,
        displayFile,
        scope,
        reason: classifyVerifyFailure(cause),
      });
    } finally {
      if (generation === verifyGeneration.current) {
        verifyInFlight.current = false;
      }
    }
  }, [current?.id, verifyState]);

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
      case COMMAND_IDS.aiAskSelection:
        setActiveTool('ai');
        if (selectedReference && aiProviderCapability.available) {
          void askAiAboutSelection(translate(preferences.locale, 'ai.defaultQuestion'));
        }
        return;
      case COMMAND_IDS.aiFixSelection:
        beginFixReview();
        return;
      case COMMAND_IDS.aiVerifyChange:
        setActiveTool('ai');
        void verifyFixChange();
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
    aiProviderCapability.available,
    askAiAboutSelection,
    beginFixReview,
    verifyFixChange,
    openNative,
    openSourceForSelection,
    patchPreferences,
    preferences.locale,
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
    <div
      className={`localview ${immersive ? 'is-immersive' : ''} ${pointSelectActive ? 'is-point-selecting' : ''} ${preferences.reducedMotion === 'reduce' ? 'is-reduced-motion' : ''}`}
      data-point-select-active={pointSelectActive || undefined}
    >
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
            mover={targetBarMover}
          />
        )}
        {preferences.showToolRail && (
          <FloatingRail
            activeTool={activeTool}
            locale={preferences.locale}
            onTool={toggleTool}
            onCommand={() => toggleTool('command')}
            mover={toolRailMover}
          />
        )}
        {activeTool && (
          <FloatingPanel
            tool={activeTool}
            state={state}
            live={live}
            actionCorrelation={actionCorrelation?.trace.action_id === latestActionId ? actionCorrelation : undefined}
            current={current}
            url={currentUrl}
            locale={preferences.locale}
            preferences={preferences}
            onClose={() => {
              if (pointSelectActive && activeTool === 'inspect') cancelPointSelect(false);
              setActiveTool(undefined);
            }}
            onSelect={setSelected}
            onOpenNative={() => void openNative()}
            responsiveState={responsiveState}
            onRunResponsiveSweep={(presets) => void captureResponsiveSweep(presets)}
            captureState={captureState}
            onCapture={() => void captureCurrentViewport()}
            sourceOpenState={sourceOpenState}
            selectedReference={selectedReference}
            onOpenSource={(reference) => void openSourceForSelection(reference)}
            measureState={measureState}
            onMeasure={(reference) => void measureCurrentSelection(reference)}
            aiProviderCapability={aiProviderCapability}
            askAiState={askAiState}
            onAskAi={(question) => void askAiAboutSelection(question)}
            onRefreshAiProvider={() => void refreshAiProviderCapability()}
            fixCapability={fixCapability}
            fixState={fixState}
            verifyState={verifyState}
            onVerifyChange={() => void verifyFixChange()}
            onBeginFix={beginFixReview}
            onPrepareFix={(instruction) => void prepareFixProposal(instruction)}
            onApplyFix={() => void applyFixProposal()}
            onDiscardFix={() => void discardFixProposal()}
            onRefreshFixCapability={() => void refreshFixCapability()}
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
  mover,
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
  mover: ChromeMover;
}) {
  const statusLabel = state.health.paused
    ? translate(locale, 'status.paused')
    : current
      ? translate(locale, 'status.ready')
      : translate(locale, 'status.offline');

  return <header ref={mover.ref} style={mover.style} className="top-pill">
    <button
      type="button"
      className="chrome-drag-handle top-pill-drag-handle"
      aria-label={translate(locale, 'aria.moveTargetBar')}
      title={translate(locale, 'aria.moveTargetBar')}
      onPointerDown={mover.onPointerDown}
      onKeyDown={mover.onKeyDown}
    ><span aria-hidden="true"/></button>
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
  mover,
}: {
  activeTool?: ToolId;
  locale: LocalViewPreferences['locale'];
  onTool: (tool: ToolId) => void;
  onCommand: () => void;
  mover: ChromeMover;
}) {
  return <nav ref={mover.ref} style={mover.style} className="floating-rail" aria-label={translate(locale, 'aria.localViewTools')}>
    <button
      type="button"
      className="chrome-drag-handle tool-rail-drag-handle"
      aria-label={translate(locale, 'aria.moveToolRail')}
      title={translate(locale, 'aria.moveToolRail')}
      onPointerDown={mover.onPointerDown}
      onKeyDown={mover.onKeyDown}
    ><span aria-hidden="true"/></button>
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
