import { useEffect, useMemo, useRef, useState } from 'react';
import { api } from '../api';
import type { Session, WorkspaceBounds, WorkspaceSurfaceSupport } from '../types';

interface WorkspaceSurfaceProps {
  current?: Session;
  url?: string;
  support: WorkspaceSurfaceSupport;
}

function readBounds(element: HTMLElement): WorkspaceBounds | undefined {
  const rect = element.getBoundingClientRect();
  if (!Number.isFinite(rect.x) || !Number.isFinite(rect.y) || !Number.isFinite(rect.width) || !Number.isFinite(rect.height)) return undefined;
  if (rect.width <= 0 || rect.height <= 0) return undefined;
  return { x: rect.x, y: rect.y, width: rect.width, height: rect.height };
}

export function WorkspaceSurface({ current, url, support }: WorkspaceSurfaceProps) {
  const slotRef = useRef<HTMLElement>(null);
  const openedSessionRef = useRef<string>();
  const lastUrlRef = useRef<string>();
  const [nativeFailedFor, setNativeFailedFor] = useState<string>();

  const wantsNative = useMemo(
    () => support.compiled && support.default_mode === 'native' && !!current && !!url && nativeFailedFor !== current.id,
    [support.compiled, support.default_mode, current?.id, url, nativeFailedFor],
  );

  useEffect(() => {
    if (!wantsNative || !current || !url) return;
    const element = slotRef.current;
    if (!element) return;

    let disposed = false;
    const sessionId = current.id;
    const initialUrl = url;
    const bounds = readBounds(element);
    if (!bounds) return;

    void api.openWorkspaceSurface(sessionId, initialUrl, bounds)
      .then(() => {
        if (disposed) {
          void api.closeWorkspaceSurface(sessionId).catch(() => undefined);
          return;
        }
        openedSessionRef.current = sessionId;
        lastUrlRef.current = initialUrl;
      })
      .catch(() => {
        if (!disposed) setNativeFailedFor(sessionId);
      });

    return () => {
      disposed = true;
      if (openedSessionRef.current === sessionId) {
        openedSessionRef.current = undefined;
        lastUrlRef.current = undefined;
        void api.closeWorkspaceSurface(sessionId).catch(() => undefined);
      }
    };
  }, [wantsNative, current?.id]);

  useEffect(() => {
    if (!wantsNative || !current || !url) return;
    if (openedSessionRef.current !== current.id || lastUrlRef.current === url) return;
    lastUrlRef.current = url;
    void api.navigateWorkspaceSurface(current.id, url).catch(() => setNativeFailedFor(current.id));
  }, [wantsNative, current?.id, url]);

  useEffect(() => {
    if (!wantsNative || !current) return;
    const element = slotRef.current;
    if (!element) return;

    let frame = 0;
    const syncBounds = () => {
      window.cancelAnimationFrame(frame);
      frame = window.requestAnimationFrame(() => {
        if (openedSessionRef.current !== current.id) return;
        const bounds = readBounds(element);
        if (!bounds) return;
        void api.setWorkspaceSurfaceBounds(current.id, bounds).catch(() => setNativeFailedFor(current.id));
      });
    };

    const observer = new ResizeObserver(syncBounds);
    observer.observe(element);
    window.addEventListener('resize', syncBounds);
    return () => {
      observer.disconnect();
      window.removeEventListener('resize', syncBounds);
      window.cancelAnimationFrame(frame);
    };
  }, [wantsNative, current?.id]);

  useEffect(() => {
    if (nativeFailedFor && nativeFailedFor !== current?.id) setNativeFailedFor(undefined);
  }, [current?.id, nativeFailedFor]);

  if (!current || !url) {
    return <main className="workspace workspace-empty">
      <div className="empty-orbit" aria-hidden="true"><i/><i/><i/><span/></div>
      <div className="empty-copy">
        <span className="micro-label">LOCALVIEW RUNTIME</span>
        <h1>Your localhost becomes the workspace.</h1>
        <p>Run a frontend dev server. LocalView discovers it automatically and keeps every analysis surface hidden until you ask for it.</p>
        <div className="empty-command"><kbd>⌘</kbd><kbd>K</kbd><span>Open command palette</span></div>
      </div>
    </main>;
  }

  const nativeActive = wantsNative;
  return <main className="workspace" ref={slotRef} data-surface={nativeActive ? 'native' : 'iframe'}>
    {!nativeActive && (
      <iframe
        key={url}
        className="app-frame"
        src={url}
        title={`${current.project.display_name} local preview`}
        referrerPolicy="no-referrer"
      />
    )}
    {nativeActive && <div className="native-surface-slot" aria-hidden="true" />}
    {current.status === 'disconnected' && <div className="disconnect-shade"><div><span className="health-dot danger"/><strong>Dev server disconnected</strong><p>LocalView is preserving the session only for the reconnect grace period.</p></div></div>}
  </main>;
}
