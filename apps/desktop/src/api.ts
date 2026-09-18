import { invoke } from '@tauri-apps/api/core';
import type { DashboardState, LiveSessionState, WorkspaceBounds } from './types';

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
  pause: () => invoke<void>('pause_runtime'),
  resume: () => invoke<void>('resume_runtime'),
  openPreview: (sessionId: string, url: string, title: string) => invoke<void>('open_preview', { sessionId, url, title }),
  captureCurrentViewport: (sessionId: string) => invoke<VisualCaptureReceipt>('capture_current_viewport', { sessionId, revision: null }),
  aiProviderCapability: () =>
    invoke<AiProviderCapability>('ai_provider_capability'),
  askAiAboutSelection: ({ sessionId, reference, question }: HumanAskAiRequest) =>
    invoke<HumanAskAiReceipt>('ask_ai_about_selection', { sessionId, reference, question }),
  openSourceForSelection: ({ sessionId, reference }: HumanSourceOpenRequest) =>
    invoke<HumanSourceOpenReceipt>('open_source_for_selection', { sessionId, reference }),
  measureElement: (sessionId: string, reference: string) => invoke<ElementMeasureReceipt>('measure_current_selection', { sessionId, reference }),
  openWorkspaceSurface: (sessionId: string, url: string, bounds: WorkspaceBounds) => invoke<void>('workspace_surface_open', { sessionId, url, bounds }),
  setWorkspaceSurfaceBounds: (sessionId: string, bounds: WorkspaceBounds) => invoke<void>('workspace_surface_set_bounds', { sessionId, bounds }),
  navigateWorkspaceSurface: (sessionId: string, url: string) => invoke<void>('workspace_surface_navigate', { sessionId, url }),
  closeWorkspaceSurface: (sessionId: string) => invoke<void>('workspace_surface_close', { sessionId }),
};
