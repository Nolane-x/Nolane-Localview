import { invoke } from '@tauri-apps/api/core';
import type { DashboardState } from './types';

export const api = {
  dashboard: () => invoke<DashboardState>('dashboard_state'),
  pause: () => invoke<void>('pause_runtime'),
  resume: () => invoke<void>('resume_runtime'),
  openPreview: (sessionId: string, url: string, title: string) => invoke<void>('open_preview', { sessionId, url, title }),
};
