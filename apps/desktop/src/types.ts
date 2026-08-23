export type SessionStatus = 'active' | 'disconnected' | 'hidden' | 'closed';

export interface Endpoint { host: string; port: number; scheme: string }
export interface Classification { kind: string; confidence: number; framework?: string; title?: string; hmr_detected: boolean; evidence: string[] }
export interface ProjectIdentity { key: string; display_name: string; cwd?: string; git_root?: string; pid?: number; command?: string }
export interface Session { id: string; endpoint: Endpoint; classification: Classification; project: ProjectIdentity; status: SessionStatus; first_seen: string; last_seen: string; disconnected_at?: string; preview_visible: boolean }
export interface Health { version: string; status: string; paused: boolean; sessions: number }
export interface DashboardState { health: Health; sessions: Session[]; engine: { native: string; tier3: string }; capabilities: string[] }
