import { invoke } from '@tauri-apps/api/core';
import type { DashboardState, LiveSessionState, WorkspaceBounds } from './types';

export const api = {
  dashboard: () => invoke<DashboardState>('dashboard_state'),
  liveSession: (sessionId: string) => invoke<LiveSessionState>('live_session_state', { sessionId }),
  pause: () => invoke<void>('pause_runtime'),
  resume: () => invoke<void>('resume_runtime'),
  openPreview: (sessionId: string, url: string, title: string) => invoke<void>('open_preview', { sessionId, url, title }),
  openWorkspaceSurface: (sessionId: string, url: string, bounds: WorkspaceBounds) => invoke<void>('workspace_surface_open', { sessionId, url, bounds }),
  setWorkspaceSurfaceBounds: (sessionId: string, bounds: WorkspaceBounds) => invoke<void>('workspace_surface_set_bounds', { sessionId, bounds }),
  navigateWorkspaceSurface: (sessionId: string, url: string) => invoke<void>('workspace_surface_navigate', { sessionId, url }),
  closeWorkspaceSurface: (sessionId: string) => invoke<void>('workspace_surface_close', { sessionId }),
};
