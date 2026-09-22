"use client";

import * as React from "react";
import { aztrayIpc } from "@/src/lib/ipc";
import type { Config, ServiceName, ServiceSnapshot, ServiceState } from "@/src/lib/types";
import type { AzTrayModel } from "@/src/hooks/useAzTray";

export const SERVICE_ORDER: ServiceName[] = ["blob", "queue", "table"];
export const SERVICE_LABELS: Record<ServiceName, string> = { blob: "Blob", queue: "Queue", table: "Table" };

export function stateLabel(state: ServiceState): string {
  return state === "portInUse" ? "Port in use" : state[0].toUpperCase() + state.slice(1);
}

export function stateTone(state: ServiceState): string {
  switch (state) {
    case "running": return "var(--status-running)";
    case "starting": return "var(--status-starting)";
    case "broken": return "var(--status-broken)";
    case "portInUse": return "var(--status-occupied)";
    default: return "var(--status-stopped)";
  }
}

function serviceSymbol(name: ServiceName) {
  return name === "blob" ? "B" : name === "queue" ? "Q" : "T";
}

function formatUptime(seconds: number | null) {
  if (seconds === null || seconds < 0) return "—";
  const hours = Math.floor(seconds / 3600);
  const minutes = Math.floor((seconds % 3600) / 60);
  const secs = Math.floor(seconds % 60);
  return hours ? `${hours}h ${String(minutes).padStart(2, "0")}m` : `${minutes}m ${String(secs).padStart(2, "0")}s`;
}

function formatTimestamp(value: string) {
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return value;
  return date.toLocaleTimeString([], { hour12: false });
}

export function StatusPill({ state, compact = false, label }: { state: ServiceState; compact?: boolean; label?: string }) {
  return (
    <span className={`status-pill status-${state}${compact ? " status-pill-compact" : ""}`}>
      <span className="status-dot" aria-hidden="true">{state === "broken" ? "×" : state === "portInUse" ? "◎" : "●"}</span>
      {label ?? stateLabel(state)}
    </span>
  );
}

export function ServiceGlyph({ name }: { name: ServiceName }) {
  return <span className={`service-glyph service-glyph-${name}`} aria-hidden="true">{serviceSymbol(name)}</span>;
}

export function EngineBanner({ model }: { model: AzTrayModel }) {
  if (model.engine.state === "ready") return null;
  const title = model.engine.state === "missingNode" ? "Node.js is missing" : model.engine.state === "missingAzurite" ? "Azurite is not available" : "Azurite needs attention";
  return (
    <section className="engine-banner" role="status">
      <span className="engine-banner-mark" aria-hidden="true">!</span>
      <div className="engine-banner-copy">
        <strong>{title}</strong>
        <span>{model.engine.message ?? "Set an executable path in Settings to enable service controls."}</span>
        {model.engine.installHint && <code>{model.engine.installHint}</code>}
      </div>
    </section>
  );
}

function ActionErrorBanner({ model }: { model: AzTrayModel }) {
  if (!model.error) return null;
  return <div className="action-error-banner" role="alert"><span>!</span><strong>Action failed</strong><span>{model.error}</span></div>;
}

export interface ServiceRowProps {
  service: ServiceSnapshot;
  selected?: boolean;
  mode?: "popover" | "sidebar";
  onSelect?: () => void;
  onStart: () => void;
  onStop: () => void;
  onRestart: () => void;
  onFreePort: () => void;
  startDisabled?: boolean;
}

export function ServiceRow({ service, selected, mode = "popover", onSelect, onStart, onStop, onRestart, onFreePort, startDisabled = false }: ServiceRowProps) {
  const isRunning = service.state === "running";
  const isStarting = service.state === "starting";
  const isOccupied = service.state === "portInUse";
  return (
    <div className={`service-row service-row-${mode}${selected ? " is-selected" : ""}`}>
      <button type="button" className="service-row-main" onClick={onSelect} aria-pressed={selected}>
        <ServiceGlyph name={service.name} />
        <span className="service-row-copy">
          <span className="service-row-title">{SERVICE_LABELS[service.name]}</span>
          <span className="service-row-endpoint">{service.host}:{service.port}</span>
        </span>
        <span className="service-row-state"><StatusPill state={service.state} compact={mode === "sidebar"} /></span>
      </button>
      {mode === "popover" && (
        <div className="service-row-action">
          {isRunning && <><button type="button" className="button button-quiet button-small" onClick={onStop}>Stop</button><button type="button" className="button button-quiet button-small" onClick={onRestart} aria-label={`Restart ${SERVICE_LABELS[service.name]}`}>↻</button></>}
          {isStarting && <button type="button" className="button button-quiet button-small" disabled>Starting</button>}
          {!isRunning && !isStarting && !isOccupied && <button type="button" className="button button-primary button-small" onClick={onStart} disabled={startDisabled}>Start</button>}
          {isOccupied && <button type="button" className="button button-danger button-small" onClick={onFreePort}>Free port</button>}
        </div>
      )}
      {mode === "sidebar" && <span className="service-row-chevron" aria-hidden="true">›</span>}
    </div>
  );
}

export function ConfirmDialog({
  title,
  body,
  confirmLabel,
  danger = true,
  onConfirm,
  onCancel,
}: {
  title: string;
  body: React.ReactNode;
  confirmLabel: string;
  danger?: boolean;
  onConfirm: () => void;
  onCancel: () => void;
}) {
  React.useEffect(() => {
    const onKey = (event: KeyboardEvent) => event.key === "Escape" && onCancel();
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onCancel]);
  return (
    <div className="dialog-backdrop" role="presentation" onMouseDown={(event) => event.target === event.currentTarget && onCancel()}>
      <section className="confirm-dialog" role="dialog" aria-modal="true" aria-labelledby="confirm-title">
        <span className="dialog-kicker">ACTION REQUIRES CONFIRMATION</span>
        <h2 id="confirm-title">{title}</h2>
        <p>{body}</p>
        <div className="dialog-actions">
          <button type="button" className="button button-quiet" onClick={onCancel}>Cancel</button>
          <button type="button" className={`button ${danger ? "button-danger" : "button-primary"}`} onClick={onConfirm}>{confirmLabel}</button>
        </div>
      </section>
    </div>
  );
}

function HeaderMark() {
  return <span className="app-mark" aria-hidden="true"><i /><i /><i /></span>;
}

function OverallStatus({ model }: { model: AzTrayModel }) {
  if (!model.services) return <span className="header-state">Connecting</span>;
  const states = SERVICE_ORDER.map((name) => model.services?.[name].state);
  const runningCount = states.filter((value) => value === "running").length;
  const state = states.includes("broken") ? "broken" : states.includes("portInUse") ? "portInUse" : states.includes("starting") ? "starting" : runningCount === states.length ? "running" : runningCount > 0 ? "starting" : "stopped";
  const label = state === "running" ? "All running" : state === "starting" && runningCount > 0 ? `${runningCount}/${states.length} running` : undefined;
  return <StatusPill state={state} label={label} />;
}

function WindowButton({ children, label, onClick, danger = false }: { children: React.ReactNode; label: string; onClick: () => void; danger?: boolean }) {
  return <button type="button" aria-label={label} title={label} onClick={onClick} className={`window-button${danger ? " window-button-danger" : ""}`}>{children}</button>;
}

export function AppHeader({ model, title, onRefresh, onClose }: { model: AzTrayModel; title: string; onRefresh?: () => void; onClose?: () => void }) {
  return (
    <header className="app-header" data-tauri-drag-region>
      <div className="app-header-brand"><HeaderMark /><span>{title}</span></div>
      <div className="app-header-status"><OverallStatus model={model} /></div>
      <div className="app-header-actions" data-tauri-drag-region="false">
        {onRefresh && <WindowButton label="Refresh status" onClick={onRefresh}>↻</WindowButton>}
        {onClose && <WindowButton label="Close window to tray" onClick={onClose}>×</WindowButton>}
      </div>
    </header>
  );
}

export function PopoverView({ model, onOpenDashboard, onHide }: { model: AzTrayModel; onOpenDashboard: () => void; onHide: () => void }) {
  const [confirm, setConfirm] = React.useState<{ title: string; body: React.ReactNode; label: string; action: () => Promise<void> } | null>(null);
  const services = model.services;
  const confirmStop = (service: ServiceSnapshot) => setConfirm({ title: `Stop ${SERVICE_LABELS[service.name]}?`, body: `AzTray will stop the app-owned process on ${service.host}:${service.port}.`, label: "Stop service", action: () => model.stop(service.name) });
  const confirmFree = (service: ServiceSnapshot) => setConfirm({ title: `Free port ${service.port}?`, body: service.portOwner ? `This will request termination of ${service.portOwner.name ?? "the process"} (PID ${service.portOwner.pid}) after identity revalidation.` : "AzTray will re-check the listener before taking action.", label: "Free port", action: () => model.freePort(service.name).then(() => undefined) });
  const runConfirm = () => {
    const action = confirm?.action;
    setConfirm(null);
    if (action) void action();
  };
  return (
    <main className="popover-window">
      <AppHeader model={model} title="AzTray" onRefresh={() => void model.refresh()} onClose={onHide} />
      <div className="popover-scroll">
        <ActionErrorBanner model={model} />
        <section className="popover-hero">
          <div><span className="eyebrow">AZURITE CONTROLLER</span><h1>{model.services ? `${model.runningCount}/3 services online` : "Local emulator"}</h1><p>{model.lastEvent}</p></div>
          <button type="button" className="button button-primary" onClick={() => void model.startAll()} disabled={!services || model.engine.state !== "ready"}>Start all</button>
        </section>
        <EngineBanner model={model} />
        <section className="service-stack" aria-label="Azurite services">
          {services ? SERVICE_ORDER.map((name) => {
            const service = services[name];
            return <ServiceRow key={name} service={service} startDisabled={model.engine.state !== "ready"} onStart={() => void model.start(name)} onStop={() => confirmStop(service)} onRestart={() => setConfirm({ title: `Restart ${SERVICE_LABELS[service.name]}?`, body: "The service will briefly stop before it starts again.", label: "Restart service", action: () => model.restart(service.name) })} onFreePort={() => confirmFree(service)} />;
          }) : <ServiceSkeleton />}
        </section>
        <section className="popover-actions">
          <button type="button" className="button button-quiet" onClick={() => setConfirm({ title: "Stop all services?", body: "AzTray will stop every app-owned Azurite process. External processes remain untouched.", label: "Stop all", action: model.stopAll })} disabled={!services}>Stop all</button>
          <button type="button" className="button button-quiet" onClick={() => setConfirm({ title: "Restart all services?", body: "All app-owned services will be restarted together.", label: "Restart all", action: model.restartAll })} disabled={!services || model.engine.state !== "ready"}>Restart all</button>
          <button type="button" className="button button-quiet" onClick={onOpenDashboard}>Open dashboard <span aria-hidden="true">↗</span></button>
        </section>
      </div>
      <footer className="app-footer"><span className="footer-event">{model.lastEvent}</span><span className="footer-address">{model.snapshot?.config.host ?? "127.0.0.1"}</span></footer>
      {confirm && <ConfirmDialog title={confirm.title} body={confirm.body} confirmLabel={confirm.label} onConfirm={runConfirm} onCancel={() => setConfirm(null)} />}
    </main>
  );
}

function ServiceSkeleton() {
  return <>{SERVICE_ORDER.map((name) => <div className="service-row service-row-popover is-loading" key={name}><span className="service-glyph" /><span className="skeleton-line skeleton-line-wide" /><span className="skeleton-line skeleton-line-short" /></div>)}</>;
}

export function DashboardView({ model, onHide }: { model: AzTrayModel; onHide: () => void }) {
  const [view, setView] = React.useState<"services" | "settings">("services");
  const [confirm, setConfirm] = React.useState<{ title: string; body: React.ReactNode; label: string; action: () => Promise<void> } | null>(null);
  const [quitOpen, setQuitOpen] = React.useState(false);
  const services = model.services;
  const selected = services?.[model.selectedService] ?? null;
  const ask = (title: string, body: React.ReactNode, label: string, action: () => Promise<void>) => setConfirm({ title, body, label, action });
  const runConfirm = () => {
    const action = confirm?.action;
    setConfirm(null);
    if (action) void action();
  };
  React.useEffect(() => {
    let dispose: (() => void) | undefined;
    let alive = true;
    void aztrayIpc.subscribe("quit_requested", () => {
      if (alive) setQuitOpen(true);
    }).then((unlisten) => {
      if (alive) dispose = unlisten;
      else unlisten();
    });
    return () => {
      alive = false;
      dispose?.();
    };
  }, []);
  return (
    <main className="dashboard-window">
      <AppHeader model={model} title="AzTray" onRefresh={() => void model.refresh()} onClose={onHide} />
      <ActionErrorBanner model={model} />
      <div className="dashboard-body">
        <nav className="dashboard-rail" aria-label="Dashboard sections">
          <button type="button" className={`rail-button${view === "services" ? " is-active" : ""}`} onClick={() => setView("services")}><span>◈</span><small>Services</small></button>
          <button type="button" className={`rail-button${view === "settings" ? " is-active" : ""}`} onClick={() => setView("settings")}><span>⌘</span><small>Settings</small></button>
        </nav>
        {view === "settings" ? <SettingsView model={model} /> : <>
          <aside className="service-sidebar">
            <div className="sidebar-heading"><span className="eyebrow">SERVICES</span><span className="sidebar-count">{model.services ? `${model.runningCount}/${SERVICE_ORDER.length} up` : "—"}</span></div>
            <div className="sidebar-list">
              {services ? SERVICE_ORDER.map((name) => <ServiceRow key={name} service={services[name]} startDisabled={model.engine.state !== "ready"} mode="sidebar" selected={name === model.selectedService} onSelect={() => model.selectService(name)} onStart={() => void model.start(name)} onStop={() => ask(`Stop ${SERVICE_LABELS[name]}?`, "The app-owned process will exit.", "Stop service", () => model.stop(name))} onRestart={() => ask(`Restart ${SERVICE_LABELS[name]}?`, "The service will briefly stop before it starts again.", "Restart service", () => model.restart(name))} onFreePort={() => ask(`Free port ${services[name].port}?`, services[name].portOwner ? `Revalidate and release PID ${services[name].portOwner.pid}.` : "Re-check the listener before taking action.", "Free port", () => model.freePort(name).then(() => undefined))} />) : <ServiceSkeleton />}
            </div>
            <div className="sidebar-bottom"><span className="eyebrow">DATA DIRECTORY</span><code>{model.snapshot?.config.dataDirectory || "Using Azurite default"}</code></div>
          </aside>
          <section className="dashboard-content">
            {selected ? <ServiceDetail model={model} service={selected} ask={ask} /> : <EmptyDashboard model={model} />}
          </section>
        </>}
      </div>
      <footer className="app-footer"><span className="footer-event">{model.lastEvent}</span><span className="footer-address">{model.snapshot?.generatedAt ? `updated ${formatTimestamp(model.snapshot.generatedAt)}` : "connecting"}</span><button type="button" className="footer-quit" onClick={() => setQuitOpen(true)}>Quit AzTray</button></footer>
      {confirm && <ConfirmDialog title={confirm.title} body={confirm.body} confirmLabel={confirm.label} onConfirm={runConfirm} onCancel={() => setConfirm(null)} />}
      {quitOpen && <QuitDialog model={model} onCancel={() => setQuitOpen(false)} onDone={onHide} />}
    </main>
  );
}

function ServiceDetail({ model, service, ask }: { model: AzTrayModel; service: ServiceSnapshot; ask: (title: string, body: React.ReactNode, label: string, action: () => Promise<void>) => void }) {
  const [logFilter, setLogFilter] = React.useState("");
  const [stream, setStream] = React.useState<"all" | "stdout" | "stderr" | "system">("all");
  const [logScope, setLogScope] = React.useState<"service" | "merged">("service");
  const [copied, setCopied] = React.useState(false);
  const allLogs = logScope === "merged" ? (model.snapshot?.mergedLogs ?? []) : (model.snapshot?.logs[service.name] ?? []);
  const logs = allLogs.filter((entry) => (stream === "all" || entry.stream === stream) && (!logFilter || entry.message.toLowerCase().includes(logFilter.toLowerCase())));
  const copyConnection = async () => {
    await model.copyConnectionString(service.name);
    setCopied(true);
    window.setTimeout(() => setCopied(false), 1400);
  };
  const copyLogs = async () => {
    const text = logs.map((entry) => `${entry.timestamp} [${entry.service}] ${entry.message}`).join("\n");
    if (navigator.clipboard) await navigator.clipboard.writeText(text);
  };
  return (
    <div className="detail-layout">
      <header className="detail-header">
        <div className="detail-heading"><ServiceGlyph name={service.name} /><div><span className="eyebrow">AZURITE SERVICE</span><h1>{SERVICE_LABELS[service.name]}</h1><span className="detail-endpoint">{service.host}:{service.port}</span></div></div>
        <StatusPill state={service.state} />
        <div className="detail-actions">
          {service.state === "running" && <button type="button" className="button button-quiet" onClick={() => ask(`Stop ${SERVICE_LABELS[service.name]}?`, "The app-owned service process will exit.", "Stop service", () => model.stop(service.name))}>Stop</button>}
          {service.state === "portInUse" && <button type="button" className="button button-danger" onClick={() => ask(`Free port ${service.port}?`, service.portOwner ? `Revalidate and release ${service.portOwner.name ?? "the listener"} (PID ${service.portOwner.pid}).` : "Re-check the listener before taking action.", "Free port", () => model.freePort(service.name).then(() => undefined))}>Free port</button>}
          {service.state !== "running" && service.state !== "starting" && service.state !== "portInUse" && <button type="button" className="button button-primary" onClick={() => void model.start(service.name)} disabled={model.engine.state !== "ready"}>Start</button>}
          {service.state === "starting" && <button type="button" className="button button-quiet" disabled>Starting…</button>}
          <button type="button" className="button button-quiet" onClick={() => ask(`Restart ${SERVICE_LABELS[service.name]}?`, "The service will briefly stop before it starts again.", "Restart service", () => model.restart(service.name))} disabled={model.engine.state !== "ready"}>Restart</button>
        </div>
      </header>
      {service.error && <div className="detail-error"><span>!</span>{service.error}</div>}
      <div className="detail-stats">
        <Stat label="PORT" value={String(service.port)} mono />
        <Stat label="PID" value={service.pid ? String(service.pid) : "—"} mono />
        <Stat label="UPTIME" value={formatUptime(service.uptimeSeconds)} mono />
        <Stat label="DATA" value={model.snapshot?.config.dataDirectory || "Azurite default"} />
      </div>
      <section className="connection-card"><div><span className="eyebrow">CONNECTION STRING</span><p>Copy a ready-to-use endpoint for this service.</p></div><button type="button" className="button button-quiet" onClick={() => void copyConnection()}>{copied ? "Copied" : "Copy string"}</button></section>
      <section className="logs-card">
        <div className="logs-heading"><div><span className="eyebrow">LIVE LOGS</span><span className="logs-count">{logs.length} lines · {logScope === "merged" ? "all services" : SERVICE_LABELS[service.name]}</span></div><div className="logs-actions"><div className="log-scope" role="group" aria-label="Log scope"><button type="button" className={logScope === "service" ? "is-active" : ""} onClick={() => setLogScope("service")}>Service</button><button type="button" className={logScope === "merged" ? "is-active" : ""} onClick={() => setLogScope("merged")}>Merged</button></div><input aria-label="Filter logs" placeholder="Filter logs" value={logFilter} onChange={(event) => setLogFilter(event.target.value)} /><select aria-label="Log stream" value={stream} onChange={(event) => setStream(event.target.value as typeof stream)}><option value="all">All streams</option><option value="stdout">stdout</option><option value="stderr">stderr</option><option value="system">system</option></select><button type="button" className="icon-action" title="Copy visible logs" aria-label="Copy visible logs" onClick={() => void copyLogs()}>⧉</button><button type="button" className="icon-action" title="Save logs" aria-label="Save logs" onClick={() => void model.saveLogs(logScope === "service" ? service.name : undefined)}>⇩</button></div></div>
        <div className="log-viewport" aria-live="polite">{logs.length ? logs.map((entry) => <div className={`log-line log-${entry.level}`} key={entry.id}><time>{formatTimestamp(entry.timestamp)}</time><span className="log-stream">{logScope === "merged" ? entry.service : entry.stream}</span><span>{entry.message}</span></div>) : <div className="empty-logs"><span>—</span><p>{service.state === "stopped" ? "Start the service to stream Azurite output." : "No matching log lines yet."}</p></div>}</div>
      </section>
    </div>
  );
}

function Stat({ label, value, mono = false }: { label: string; value: string; mono?: boolean }) {
  return <div className="stat-cell"><span className="eyebrow">{label}</span><strong className={mono ? "mono" : ""}>{value}</strong></div>;
}

function EmptyDashboard({ model }: { model: AzTrayModel }) {
  return <div className="empty-dashboard"><HeaderMark /><h1>Azurite at a glance</h1><p>Select a service to see its status and live logs.</p><button type="button" className="button button-primary" onClick={() => void model.startAll()} disabled={model.engine.state !== "ready"}>Start all services</button></div>;
}

function QuitDialog({ model, onCancel, onDone }: { model: AzTrayModel; onCancel: () => void; onDone: () => void }) {
  const [busy, setBusy] = React.useState(false);
  const choose = async (mode: "stop_and_quit" | "leave_running") => {
    setBusy(true);
    await model.quit(mode);
    onDone();
  };
  return <div className="dialog-backdrop"><section className="confirm-dialog quit-dialog" role="dialog" aria-modal="true"><span className="dialog-kicker">QUIT AZTRAY</span><h2>What should happen to Azurite?</h2><p>Choose whether app-owned services continue after the tray controller closes.</p><div className="quit-options"><button type="button" className="quit-option" disabled={busy} onClick={() => void choose("stop_and_quit")}><strong>Stop &amp; quit</strong><span>Stop every service AzTray started, then exit.</span></button><button type="button" className="quit-option" disabled={busy} onClick={() => void choose("leave_running")}><strong>Leave running</strong><span>Keep Azurite available while AzTray exits.</span></button></div><button type="button" className="button button-quiet dialog-cancel" onClick={onCancel} disabled={busy}>Cancel</button></section></div>;
}

function SettingsView({ model }: { model: AzTrayModel }) {
  const current = model.snapshot?.config;
  const [draft, setDraft] = React.useState<Config | null>(current ?? null);
  React.useEffect(() => { if (current) setDraft(current); }, [current]);
  if (!draft) return <div className="loading-panel">Loading settings…</div>;
  const updatePort = (service: ServiceName, value: string) => setDraft({ ...draft, ports: { ...draft.ports, [service]: Number(value) || 0 } });
  return <section className="settings-view"><div className="settings-heading"><div><span className="eyebrow">CONTROLLER SETTINGS</span><h1>Azurite connection</h1><p>Changes apply to the next service start.</p></div><button type="button" className="button button-primary" onClick={() => void model.saveConfig(draft)}>Save settings</button></div><EngineBanner model={model} /><div className="settings-card"><label><span className="eyebrow">HOST</span><input value={draft.host} onChange={(event) => setDraft({ ...draft, host: event.target.value })} /></label><div className="settings-grid"><label><span className="eyebrow">BLOB PORT</span><input inputMode="numeric" value={draft.ports.blob} onChange={(event) => updatePort("blob", event.target.value)} /></label><label><span className="eyebrow">QUEUE PORT</span><input inputMode="numeric" value={draft.ports.queue} onChange={(event) => updatePort("queue", event.target.value)} /></label><label><span className="eyebrow">TABLE PORT</span><input inputMode="numeric" value={draft.ports.table} onChange={(event) => updatePort("table", event.target.value)} /></label></div><label><span className="eyebrow">DATA DIRECTORY</span><input value={draft.dataDirectory} placeholder="Azurite default" onChange={(event) => setDraft({ ...draft, dataDirectory: event.target.value })} /></label><label><span className="eyebrow">AZURITE EXECUTABLE OVERRIDE</span><input value={draft.executablePath ?? ""} placeholder="Use Node/npm discovery" onChange={(event) => setDraft({ ...draft, executablePath: event.target.value || null })} /></label><label><span className="eyebrow">NODE EXECUTABLE OVERRIDE</span><input value={draft.nodePath ?? ""} placeholder="Use PATH discovery" onChange={(event) => setDraft({ ...draft, nodePath: event.target.value || null })} /></label></div></section>;
}
