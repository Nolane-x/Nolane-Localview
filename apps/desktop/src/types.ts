export type SessionStatus = 'active' | 'disconnected' | 'hidden' | 'closed';
export type ObserverEventKind = 'dom_mutation' | 'layout' | 'route' | 'focus' | 'scroll' | 'console' | 'network' | 'runtime_error' | 'performance' | 'hmr' | 'semantic_snapshot';
export type WorkspaceSurfaceMode = 'iframe' | 'native';

export interface Endpoint { host: string; port: number; scheme: string }
export interface Classification { kind: string; confidence: number; framework?: string; title?: string; hmr_detected: boolean; evidence: string[] }
export interface ProjectIdentity { key: string; display_name: string; cwd?: string; git_root?: string; pid?: number; command?: string }
export interface Session { id: string; endpoint: Endpoint; classification: Classification; project: ProjectIdentity; status: SessionStatus; first_seen: string; last_seen: string; disconnected_at?: string; preview_visible: boolean }
export interface Health { version: string; status: string; paused: boolean; sessions: number }
export interface WorkspaceBounds { x: number; y: number; width: number; height: number }
export interface WorkspaceSurfaceSupport { compiled: boolean; default_mode: WorkspaceSurfaceMode; reason: string }
export interface DashboardState { health: Health; sessions: Session[]; engine: { native: string; tier3: string }; capabilities: string[]; workspace_surface: WorkspaceSurfaceSupport }

export interface ObserverEvent {
  seq: number;
  captured_at: string;
  kind: ObserverEventKind;
  reference?: string;
  route?: string;
  payload: Record<string, unknown>;
}

export interface BridgeActionResult {
  action_id: string;
  ok: boolean;
  error?: string;
  payload: unknown;
  completed_at: string;
}

export interface LiveSessionState {
  observer: ObserverEvent[];
  action_results: BridgeActionResult[];
}
