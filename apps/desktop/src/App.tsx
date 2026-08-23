import { useCallback, useEffect, useMemo, useState } from 'react';
import { api } from './api';
import type { DashboardState, Session } from './types';

const fallback: DashboardState = {
  health: { version: '0.2.0', status: 'connecting', paused: false, sessions: 0 },
  sessions: [],
  engine: { native: 'Tauri / WRY', tier3: 'Chromium on demand' },
  capabilities: ['Discovery', 'Sessions', 'Semantic diff', 'Layout', 'Visual', 'Responsive', 'A11y', 'MCP'],
};

export default function App() {
  const [state, setState] = useState<DashboardState>(fallback);
  const [selected, setSelected] = useState<string>();
  const [error, setError] = useState<string>();
  const [tab, setTab] = useState<'overview' | 'runtime' | 'agent'>('overview');

  const refresh = useCallback(async () => {
    try {
      const next = await api.dashboard();
      setState(next); setError(undefined);
      if (!selected && next.sessions[0]) setSelected(next.sessions[0].id);
    } catch (e) { setError(String(e)); }
  }, [selected]);

  useEffect(() => { void refresh(); const timer = window.setInterval(() => void refresh(), 1400); return () => window.clearInterval(timer); }, [refresh]);
  const current = useMemo(() => state.sessions.find((s) => s.id === selected) ?? state.sessions[0], [state.sessions, selected]);

  async function togglePause() { state.health.paused ? await api.resume() : await api.pause(); await refresh(); }
  async function open(session: Session) { await api.openPreview(session.id, `${session.endpoint.scheme}://${session.endpoint.host}:${session.endpoint.port}/`, session.project.display_name); }

  return <div className="shell">
    <aside className="sidebar">
      <div className="brand"><div className="brand-mark">LV</div><div><strong>LocalView</strong><span>AI-native localhost</span></div></div>
      <nav className="nav">
        <button className={tab === 'overview' ? 'active' : ''} onClick={() => setTab('overview')}>Overview <kbd>1</kbd></button>
        <button className={tab === 'runtime' ? 'active' : ''} onClick={() => setTab('runtime')}>Runtime <kbd>2</kbd></button>
        <button className={tab === 'agent' ? 'active' : ''} onClick={() => setTab('agent')}>Agent plane <kbd>3</kbd></button>
      </nav>
      <div className="sidebar-section-label">Sessions</div>
      <div className="session-list">
        {state.sessions.length === 0 && <div className="empty-mini">No localhost UI detected</div>}
        {state.sessions.map((s) => <button key={s.id} onClick={() => setSelected(s.id)} className={`session-row ${current?.id === s.id ? 'selected' : ''}`}>
          <i className={`status-dot ${s.status}`} /><span className="session-name">{s.project.display_name}</span><span className="port">:{s.endpoint.port}</span>
        </button>)}
      </div>
      <div className="sidebar-footer"><span className={`daemon-dot ${error ? 'bad' : ''}`} />{error ? 'Daemon unreachable' : `Daemon ${state.health.status}`}</div>
    </aside>

    <main className="main">
      <header className="topbar">
        <div><p className="eyebrow">LOCAL RUNTIME</p><h1>{tab === 'overview' ? 'Workspace' : tab === 'runtime' ? 'Runtime topology' : 'AI control plane'}</h1></div>
        <div className="top-actions"><button className="ghost" onClick={() => void refresh()}>Refresh</button><button className={state.health.paused ? 'accent paused' : 'accent'} onClick={() => void togglePause()}>{state.health.paused ? 'Resume detection' : 'Pause detection'}</button></div>
      </header>

      {tab === 'overview' && <Overview state={state} current={current} open={open} error={error} />}
      {tab === 'runtime' && <Runtime state={state} current={current} />}
      {tab === 'agent' && <Agent state={state} />}
    </main>
  </div>;
}

function Overview({ state, current, open, error }: { state: DashboardState; current?: Session; open: (s: Session) => Promise<void>; error?: string }) {
  return <div className="content">
    {error && <div className="alert"><strong>Daemon unavailable.</strong><span>{error}</span></div>}
    <section className="metrics">
      <Metric label="Live sessions" value={String(state.sessions.filter((s) => s.status !== 'disconnected').length).padStart(2, '0')} detail="auto-discovered" />
      <Metric label="Runtime" value={state.health.paused ? 'Paused' : 'Active'} detail={`v${state.health.version}`} />
      <Metric label="Native engine" value="Tier 2" detail={state.engine.native} />
      <Metric label="Agent surface" value={String(state.capabilities.length)} detail="capability families" />
    </section>

    <section className="grid two">
      <div className="panel session-panel">
        <PanelTitle kicker="CURRENT TARGET" title={current ? current.project.display_name : 'Waiting for localhost'} badge={current?.classification.framework ?? 'Discovery'} />
        {current ? <>
          <div className="preview-placeholder">
            <div className="urlbar"><span className="traffic" /><span>{current.endpoint.scheme}://{current.endpoint.host}:{current.endpoint.port}</span><em>{Math.round(current.classification.confidence * 100)}% confidence</em></div>
            <div className="preview-center"><div className="scan-rings"><span /><span /><span /></div><strong>Native preview surface</strong><p>Open the detected app in an isolated Tauri WebView.</p><button onClick={() => void open(current)}>Open preview</button></div>
          </div>
          <div className="facts"><Fact label="Framework" value={current.classification.framework ?? 'Generic web'} /><Fact label="HMR" value={current.classification.hmr_detected ? 'Detected' : 'Not detected'} /><Fact label="Project" value={current.project.cwd ?? 'process-derived'} /></div>
        </> : <div className="waiting"><div className="radar" /><h3>Run any local web app</h3><p>LocalView watches loopback listeners and classifies frontend candidates without wrapping your dev command.</p></div>}
      </div>

      <div className="panel stack-panel">
        <PanelTitle kicker="PERCEPTION FUSION" title="Four-sense runtime" badge="Live architecture" />
        <div className="sense-list">
          <Sense n="01" title="Vision" text="Native screenshots, regions, progressive capture and pixel-aware diff." />
          <Sense n="02" title="Structure" text="DOM/AX semantics, geometry, source hints and stable element references." />
          <Sense n="03" title="Behavior" text="Interactions, deterministic flow graph and replay primitives." />
          <Sense n="04" title="Telemetry" text="Console, network, HMR and performance signals merged as evidence." />
        </div>
      </div>
    </section>

    <section className="panel capability-panel"><PanelTitle kicker="SYSTEM" title="Capability mesh" badge="Rust workspace" />
      <div className="cap-grid">{state.capabilities.map((c, i) => <div className="cap" key={c}><span>{String(i + 1).padStart(2, '0')}</span><strong>{c}</strong><i /></div>)}</div>
    </section>
  </div>;
}

function Runtime({ state, current }: { state: DashboardState; current?: Session }) {
  return <div className="content"><section className="panel topology"><PanelTitle kicker="ENGINE ESCALATION" title="Light by default. Heavy only by evidence." badge={state.engine.native} />
    <div className="tier-row"><Tier n="0" title="Static / Source" desc="routes · config · source" /><Arrow /><Tier n="1" title="Machine Runtime" desc="semantics · interaction" /><Arrow /><Tier n="2" title="Native WebView" desc="human render · capture" active /><Arrow /><Tier n="3" title="Chromium" desc="compat · DevTools trace" /></div>
  </section>
  <section className="grid two"><div className="panel"><PanelTitle kicker="SELECTED SESSION" title={current?.project.display_name ?? 'None'} badge={current ? `:${current.endpoint.port}` : 'idle'} />
    <pre className="json">{JSON.stringify(current ?? { status: 'No detected frontend' }, null, 2)}</pre></div>
    <div className="panel"><PanelTitle kicker="LIFECYCLE" title="Automatic cleanup" badge="anti-zombie" /><div className="timeline"><Timeline t="01" text="Listener appears → HTTP probe"/><Timeline t="02" text="Frontend classified → session identity"/><Timeline t="03" text="HMR / runtime observations → diff"/><Timeline t="04" text="Port disappears → grace period"/><Timeline t="05" text="No reconnect → surfaces released"/></div></div></section></div>;
}

function Agent({ state }: { state: DashboardState }) {
  return <div className="content"><section className="grid agent-grid"><div className="panel"><PanelTitle kicker="MCP / CLI / SDK" title="One control plane" badge="localhost only" /><div className="terminal"><div>$ localview sessions</div><div className="dim"># compact session inventory</div><br/><div>$ localview-mcp</div><div className="dim"># stdio JSON-RPC bridge</div><br/><div>$ localview diagnose</div><div className="dim"># planned evidence-ranked diagnosis</div></div></div>
    <div className="panel"><PanelTitle kicker="TOKEN ECONOMY" title="Don't send what didn't change" badge="diff-first" /><div className="budget"><span>STATE 184 → 185</span><strong>2 nodes changed</strong><p>Unchanged viewport, accessibility, console and routes are referenced instead of re-serialized.</p><div className="budget-bar"><i style={{width:'18%'}} /></div><em>18% packet budget used</em></div></div></section>
    <section className="panel"><PanelTitle kicker="AVAILABLE NOW" title="Agent capability families" badge={`${state.capabilities.length} systems`} /><div className="command-grid">{state.capabilities.map(c=><code key={c}>localview::{c.toLowerCase().replaceAll(' ','_')}</code>)}</div></section></div>;
}

function Metric({ label, value, detail }: { label: string; value: string; detail: string }) { return <div className="metric"><span>{label}</span><strong>{value}</strong><em>{detail}</em></div>; }
function PanelTitle({ kicker, title, badge }: { kicker: string; title: string; badge: string }) { return <div className="panel-title"><div><p>{kicker}</p><h2>{title}</h2></div><span>{badge}</span></div>; }
function Fact({ label, value }: { label: string; value: string }) { return <div><span>{label}</span><strong title={value}>{value}</strong></div>; }
function Sense({ n, title, text }: { n: string; title: string; text: string }) { return <div className="sense"><span>{n}</span><div><strong>{title}</strong><p>{text}</p></div></div>; }
function Tier({ n, title, desc, active }: { n: string; title: string; desc: string; active?: boolean }) { return <div className={`tier ${active ? 'tier-active' : ''}`}><span>TIER {n}</span><strong>{title}</strong><p>{desc}</p></div>; }
function Arrow() { return <div className="arrow">→</div>; }
function Timeline({ t, text }: { t: string; text: string }) { return <div className="timeline-row"><span>{t}</span><i /><p>{text}</p></div>; }
