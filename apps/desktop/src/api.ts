import { invoke } from '@tauri-apps/api/core';
import type { ActionCorrelationReceipt, DashboardState, LiveSessionState, WorkspaceBounds } from './types';

export interface MeasureRect {
  x: number;
  y: number;
  width: number;
  height: number;
}

export interface AiProviderCapability {
  available: boolean;
  label?: string | null;
  reason?: 'not_configured' | 'unsupported' | null;
}

export interface HumanAskAiRequest {
  sessionId: string;
  reference: string;
  question: string;
}

export interface HumanAskAiReceipt {
  reference: string;
  answer: string;
  providerLabel: string;
  contextVersion: number;
  snapshotVersion: number;
  completedAtUnixMs: number;
}

export interface AiFixCapability {
  available: boolean;
  providerLabel?: string | null;
  reason?: 'not_enabled' | 'provider_unavailable' | null;
}

export interface HumanFixProposalRequest {
  sessionId: string;
  reference: string;
  instruction: string;
}

export interface HumanFixProposalReceipt {
  proposalId: string;
  reference: string;
  displayFile: string;
  summary: string;
  diff: string;
  providerLabel: string;
  expiresAtUnixMs: number;
}

export interface HumanApplyFixRequest {
  proposalId: string;
}

export type VerifyScope = 'semantic_visual' | 'semantic_only';

export type VerifyStatus =
  | 'change_observed'
  | 'no_observable_change'
  | 'regression_signal'
  | 'inconclusive';

export type VerifyVisualChangeMode = 'unchanged' | 'regions' | 'viewport';

export interface HumanApplyFixReceipt {
  proposalId: string;
  reference: string;
  displayFile: string;
  applied: true;
  changedStartLine: number;
  changedEndLine: number;
  verificationId: string;
  verificationScope: VerifyScope;
  appliedAtUnixMs: number;
}

export interface HumanVerifyChangeRequest {
  verificationId: string;
}

export interface Wave9BoundedVerificationReceipt {
  schema_version: number;
  scope: 'current_target_current_route';
  canonical_route: string;
  reference?: string | null;
  snapshot_version: number;
  verdict: 'verified' | 'rejected' | 'inconclusive';
  reasons: string[];
  evidence_ids: string[];
}

export interface Wave9AutonomousReceipt {
  final_verdict: 'verified' | 'rejected' | 'inconclusive';
  bounded_verification?: Wave9BoundedVerificationReceipt | null;
  reasons: string[];
  evidence_ids: string[];
  stale_evidence_ids: string[];
  revalidated_state_set: string[];
  unexpected_impact: Array<{ kind: string; id: string }>;
}

export interface HumanVerifyChangeReceipt {
  verificationId: string;
  reference: string;
  displayFile: string;
  scope: VerifyScope;
  status: VerifyStatus;
  semanticChanges: string[];
  regressionSignals: string[];
  viewportChangedRatio?: number | null;
  targetChangedRatio?: number | null;
  visualChangeMode?: VerifyVisualChangeMode | null;
  affectedRegions: MeasureRect[];
  affectedVisualEvidenceIds: string[];
  visualDiffEvidenceId?: string | null;
  snapshotVersion: number;
  providerLabel?: string | null;
  advisorySummary?: string | null;
  wave9Autonomous?: Wave9AutonomousReceipt | null;
  wave9AutonomousError?: string | null;
  verifiedAtUnixMs: number;
}

export type HumanPointSelectPhase = 'pending' | 'selected' | 'cancelled' | 'failed' | 'stale';

export interface HumanPointSelectStatus {
  sessionId: string;
  requestToken: string;
  route: string;
  state: HumanPointSelectPhase;
  reference?: string | null;
  bridgeGeneration?: number | null;
  reason?: string | null;
}

export interface HumanSourceOpenRequest {
  sessionId: string;
  reference: string;
}

export interface HumanSourceOpenReceipt {
  reference: string;
  displayFile: string;
  line: number;
  column?: number | null;
  launcher: 'mac_open' | 'linux_xdg_open' | 'windows_file_protocol_handler';
  snapshotVersion: number;
}

export interface ElementMeasureReceipt {
  reference: string;
  rect: MeasureRect;
  document_rect: MeasureRect;
  viewport_css_width: number;
  viewport_css_height: number;
  route: string;
  measured_at_unix_ms: number;
}

export type ResponsivePresetId = 'mobile_s' | 'mobile' | 'tablet' | 'desktop';

export interface ResponsiveSweepRequest {
  sessionId: string;
  presets: ResponsivePresetId[];
}

export interface ResponsiveSweepReceipt {
  artifact_id: string;
  evidence_id: string;
  deduplicated: boolean;
  route: string;
  contact_sheet_pixel_width: number;
  contact_sheet_pixel_height: number;
  viewports: ResponsiveViewportReceipt[];
}

export interface ResponsiveViewportReceipt {
  preset: ResponsivePresetId;
  css_width: number;
  css_height: number;
  device_scale_factor: number;
  pixel_width: number;
  pixel_height: number;
  sheet_x: number;
  sheet_y: number;
}

export type ContentStressProfile =
  | 'expanded_130'
  | 'expanded_180'
  | 'dense_cjk'
  | 'rtl_pseudo';

export interface ContentStressIssue {
  profile: ContentStressProfile;
  code: string;
  refs: string[];
  confidence: number;
  evidence: string;
}

export interface ContentStressProfileReceipt {
  profile: ContentStressProfile;
  synthetic: boolean;
  mutated_nodes: number;
  snapshot_version: number;
  issue_count: number;
}

export interface ContentStressReceipt {
  route: string;
  viewport: [number, number];
  synthetic: boolean;
  restored: boolean;
  profiles: ContentStressProfileReceipt[];
  issues: ContentStressIssue[];
}

export interface VisualCaptureReceipt {
  artifact_id: string;
  evidence_id: string;
  deduplicated: boolean;
  backend: string;
  route: string;
  viewport: {
    css_width: number;
    css_height: number;
    device_scale_factor: number;
  };
  pixel_width: number;
  pixel_height: number;
  revision?: string | null;
  captured_at_unix_ms: number;
  target: string;
  region?: { x: number; y: number; width: number; height: number } | null;
}

export const api = {
  dashboard: () => invoke<DashboardState>('dashboard_state'),
  liveSession: (sessionId: string) => invoke<LiveSessionState>('live_session_state', { sessionId }),
  actionCorrelation: (sessionId: string, actionId: string) =>
    invoke<ActionCorrelationReceipt | null>('action_correlation', { sessionId, actionId }),
  pause: () => invoke<void>('pause_runtime'),
  resume: () => invoke<void>('resume_runtime'),
  openPreview: (sessionId: string, url: string, title: string) => invoke<void>('open_preview', { sessionId, url, title }),
  aiProviderCapability: () =>
    invoke<AiProviderCapability>('ai_provider_capability'),
  askAiAboutSelection: ({ sessionId, reference, question }: HumanAskAiRequest) =>
    invoke<HumanAskAiReceipt>('ask_ai_about_selection', { sessionId, reference, question }),
  aiFixCapability: () =>
    invoke<AiFixCapability>('ai_fix_capability'),
  prepareFixProposal: ({ sessionId, reference, instruction }: HumanFixProposalRequest) =>
    invoke<HumanFixProposalReceipt>('prepare_fix_proposal', { sessionId, reference, instruction }),
  applyFixProposal: ({ proposalId }: HumanApplyFixRequest) =>
    invoke<HumanApplyFixReceipt>('apply_fix_proposal', { proposalId }),
  discardFixProposal: ({ proposalId }: HumanApplyFixRequest) =>
    invoke<void>('discard_fix_proposal', { proposalId }),
  verifyFixChange: ({ verificationId }: HumanVerifyChangeRequest) =>
    invoke<HumanVerifyChangeReceipt>('verify_fix_change', { verificationId }),
  captureCurrentViewport: (sessionId: string) => invoke<VisualCaptureReceipt>('capture_current_viewport', { sessionId, revision: null }),
  captureResponsiveSweep: ({ sessionId, presets }: ResponsiveSweepRequest) =>
    invoke<ResponsiveSweepReceipt>('capture_responsive_sweep', { sessionId, presets }),
  captureContentLocaleStress: (sessionId: string) =>
    invoke<ContentStressReceipt>('capture_content_locale_stress', { sessionId }),
  beginPointSelect: (sessionId: string, requestToken: string) =>
    invoke<HumanPointSelectStatus>('point_select_begin', { sessionId, requestToken }),
  pointSelectStatus: (sessionId: string, requestToken: string) =>
    invoke<HumanPointSelectStatus>('point_select_status', { sessionId, requestToken }),
  cancelPointSelect: (sessionId: string, requestToken: string) =>
    invoke<HumanPointSelectStatus>('point_select_cancel', { sessionId, requestToken }),
  openSourceForSelection: ({ sessionId, reference }: HumanSourceOpenRequest) =>
    invoke<HumanSourceOpenReceipt>('open_source_for_selection', { sessionId, reference }),
  measureElement: (sessionId: string, reference: string) => invoke<ElementMeasureReceipt>('measure_current_selection', { sessionId, reference }),
  openWorkspaceSurface: (sessionId: string, url: string, bounds: WorkspaceBounds) => invoke<void>('workspace_surface_open', { sessionId, url, bounds }),
  setWorkspaceSurfaceBounds: (sessionId: string, bounds: WorkspaceBounds) => invoke<void>('workspace_surface_set_bounds', { sessionId, bounds }),
  navigateWorkspaceSurface: (sessionId: string, url: string) => invoke<void>('workspace_surface_navigate', { sessionId, url }),
  closeWorkspaceSurface: (sessionId: string) => invoke<void>('workspace_surface_close', { sessionId }),
};
