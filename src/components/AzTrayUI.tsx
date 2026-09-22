"use client";

import * as React from "react";
import {
  Activity,
  AlertCircle,
  ArrowUpRight,
  Box,
  Check,
  ChevronRight,
  CircleHelp,
  Clipboard,
  Cloud,
  Copy,
  Database,
  FileDown,
  FolderOpen,
  LayoutDashboard,
  ListChecks,
  LoaderCircle,
  Moon,
  PackageOpen,
  Play,
  Power,
  RefreshCw,
  RotateCw,
  Server,
  Settings2,
  Square,
  Sun,
  Table2,
  TerminalSquare,
  Zap,
  X,
} from "lucide-react";
import { AlertDialog, AlertDialogAction, AlertDialogCancel, AlertDialogContent, AlertDialogDescription, AlertDialogFooter, AlertDialogHeader, AlertDialogTitle } from "@/src/components/ui/alert-dialog";
import { Badge } from "@/src/components/ui/badge";
import { Button } from "@/src/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/src/components/ui/card";
import { Input } from "@/src/components/ui/input";
import { ScrollArea } from "@/src/components/ui/scroll-area";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/src/components/ui/select";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/src/components/ui/tabs";
import { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger } from "@/src/components/ui/tooltip";
import { aztrayIpc } from "@/src/lib/ipc";
import type { Config, ServiceName, ServiceSnapshot, ServiceState } from "@/src/lib/types";
import type { AzTrayModel } from "@/src/hooks/useAzTray";

const THEME_STORAGE_KEY = "aztray-theme";
const THEME_CHANNEL_NAME = "aztray-theme";
type ThemeMode = "light" | "dark" | "system";
type EffectiveTheme = "light" | "dark";

function isThemeMode(value: string | null | undefined): value is ThemeMode {
  return value === "light" || value === "dark" || value === "system";
}

function systemTheme(): EffectiveTheme {
  return typeof window !== "undefined" && window.matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "light";
}

function initialThemeMode(): ThemeMode {
  if (typeof document === "undefined") return "system";
  const mode = document.documentElement.dataset.themeMode;
  return isThemeMode(mode) ? mode : "system";
}

function initialEffectiveTheme(): EffectiveTheme {
  if (typeof document !== "undefined") {
    const theme = document.documentElement.dataset.theme;
    if (theme === "light" || theme === "dark") return theme;
  }
  return systemTheme();
}

export function useTheme() {
  const [mode, setModeState] = React.useState<ThemeMode>(initialThemeMode);
  const [theme, setThemeState] = React.useState<EffectiveTheme>(initialEffectiveTheme);
  const channelRef = React.useRef<BroadcastChannel | null>(null);

  const applyTheme = React.useCallback((next: ThemeMode) => {
    const effective = next === "system" ? systemTheme() : next;
    document.documentElement.dataset.themeMode = next;
    document.documentElement.dataset.theme = effective;
    document.documentElement.style.colorScheme = effective;
    setThemeState(effective);
  }, []);

  React.useEffect(() => {
    applyTheme(mode);
    const receive = (next: ThemeMode) => { setModeState(next); applyTheme(next); };
    const onStorage = (event: StorageEvent) => { if (event.key === THEME_STORAGE_KEY && isThemeMode(event.newValue)) receive(event.newValue); };
    const media = window.matchMedia("(prefers-color-scheme: dark)");
    const onSystemChange = () => { if (mode === "system") applyTheme("system"); };
    window.addEventListener("storage", onStorage);
    media.addEventListener?.("change", onSystemChange);
    if ("BroadcastChannel" in window) {
      const channel = new BroadcastChannel(THEME_CHANNEL_NAME);
      channelRef.current = channel;
      channel.onmessage = (event: MessageEvent<unknown>) => {
        const next = typeof event.data === "string" ? event.data : (event.data as { mode?: unknown } | null)?.mode;
        if (typeof next === "string" && isThemeMode(next)) receive(next);
      };
    }
    return () => {
      window.removeEventListener("storage", onStorage);
      media.removeEventListener?.("change", onSystemChange);
      channelRef.current?.close();
      channelRef.current = null;
    };
  }, [applyTheme, mode]);

  const setMode = React.useCallback((next: ThemeMode) => {
    setModeState(next);
    applyTheme(next);
    try { window.localStorage.setItem(THEME_STORAGE_KEY, next); } catch { /* previews may disable storage */ }
    try { document.cookie = `${THEME_STORAGE_KEY}=${next}; max-age=31536000; path=/; SameSite=Lax`; } catch { /* previews may disable cookies */ }
    channelRef.current?.postMessage(next);
  }, [applyTheme]);

  const toggle = React.useCallback(() => setMode(theme === "dark" ? "light" : "dark"), [setMode, theme]);
  return { mode, theme, setMode, toggle };
}

export const SERVICE_ORDER: ServiceName[] = ["blob", "queue", "table"];
export const SERVICE_LABELS: Record<ServiceName, string> = { blob: "Blob", queue: "Queue", table: "Table" };

const SERVICE_META: Record<ServiceName, { description: string; accent: string; icon: React.ComponentType<{ className?: string }> }> = {
  blob: { description: "Object storage", accent: "blob", icon: Box },
  queue: { description: "Message queues", accent: "queue", icon: ListChecks },
  table: { description: "NoSQL tables", accent: "table", icon: Table2 },
};

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

function statusIcon(state: ServiceState) {
  if (state === "starting") return <LoaderCircle className="az-status-icon az-spin" aria-hidden="true" />;
  if (state === "broken") return <X className="az-status-icon" aria-hidden="true" />;
  if (state === "portInUse") return <AlertCircle className="az-status-icon" aria-hidden="true" />;
  if (state === "running") return <Check className="az-status-icon" aria-hidden="true" />;
  return <span className="az-status-dot" aria-hidden="true" />;
}

export function StatusPill({ state, compact = false, label }: { state: ServiceState; compact?: boolean; label?: string }) {
  return <Badge variant="outline" className={`az-status-pill az-status-${state}${compact ? " az-status-compact" : ""}`}>{statusIcon(state)}<span>{label ?? stateLabel(state)}</span></Badge>;
}

export function ServiceGlyph({ name }: { name: ServiceName }) {
  const Icon = SERVICE_META[name].icon;
  return <span className={`az-service-icon az-service-icon-${SERVICE_META[name].accent}`} aria-hidden="true"><Icon className="az-service-icon-svg" /></span>;
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

function overallState(model: AzTrayModel): ServiceState {
  if (!model.services) return "starting";
  const states = SERVICE_ORDER.map((name) => model.services?.[name].state);
  const running = states.filter((state) => state === "running").length;
  if (states.includes("broken")) return "broken";
  if (states.includes("portInUse")) return "portInUse";
  if (states.includes("starting")) return "starting";
  if (running === states.length) return "running";
  return "stopped";
}

function HeaderMark() {
  return <span className="az-app-mark" aria-hidden="true"><Activity className="az-app-mark-icon" /></span>;
}

function ThemeToggle() {
  const { mode, theme, toggle } = useTheme();
  const nextLabel = theme === "dark" ? "Switch to light mode" : "Switch to dark mode";
  return <Tooltip><TooltipTrigger asChild><Button type="button" variant="ghost" size="icon" className="az-icon-button" suppressHydrationWarning onClick={toggle} aria-label={`${nextLabel} (${mode})`}><span suppressHydrationWarning>{theme === "dark" ? <Sun /> : <Moon />}</span></Button></TooltipTrigger><TooltipContent>{nextLabel}</TooltipContent></Tooltip>;
}

function WindowButton({ icon, label, onClick, danger = false }: { icon: React.ReactNode; label: string; onClick: () => void; danger?: boolean }) {
  return <Tooltip><TooltipTrigger asChild><Button type="button" variant="ghost" size="icon" aria-label={label} onClick={onClick} className={`az-icon-button${danger ? " az-icon-button-danger" : ""}`}>{icon}</Button></TooltipTrigger><TooltipContent>{label}</TooltipContent></Tooltip>;
}

export function AppHeader({ model, title, onRefresh, onClose }: { model: AzTrayModel; title: string; onRefresh?: () => void; onClose?: () => void }) {
  const state = overallState(model);
  const runningCopy = !model.services ? "Connecting" : state === "running" ? "All services online" : `${model.runningCount} of ${SERVICE_ORDER.length} services online`;
  return <header className="az-titlebar" data-tauri-drag-region>
    <div className="az-titlebar-brand"><HeaderMark /><div><strong>{title}</strong><span>Azurite controller</span></div></div>
    <div className="az-titlebar-state"><StatusPill state={state} label={runningCopy} compact /></div>
    <div className="az-titlebar-actions" data-tauri-drag-region="false"><ThemeToggle />{onRefresh && <WindowButton label="Refresh status" onClick={onRefresh} icon={<RefreshCw />} />}{onClose && <WindowButton label="Hide to tray" onClick={onClose} icon={<X />} />}</div>
  </header>;
}

export function EngineBanner({ model }: { model: AzTrayModel }) {
  if (model.engine.state === "ready") return null;
  const title = model.engine.state === "missingNode" ? "Node.js is missing" : model.engine.state === "missingAzurite" ? "Azurite is not available" : "Azurite needs attention";
  return <Card className="az-engine-banner" role="status"><div className="az-engine-icon"><AlertCircle /></div><div className="az-engine-copy"><strong>{title}</strong><span>{model.engine.message ?? "Set an executable path in Settings to enable service controls."}</span>{model.engine.installHint && <code>{model.engine.installHint}</code>}</div><Settings2 className="az-engine-end-icon" /></Card>;
}

function ActionErrorBanner({ model }: { model: AzTrayModel }) {
  if (!model.error) return null;
  return <div className="az-action-error" role="alert"><AlertCircle /><strong>Action failed</strong><span>{model.error}</span></div>;
}

export interface ServiceRowProps {
  service: ServiceSnapshot;
  selected?: boolean;
  mode?: "popover" | "dashboard";
  onSelect?: () => void;
  onStart: () => void;
  onStop: () => void;
  onRestart: () => void;
  onFreePort: () => void;
  startDisabled?: boolean;
}

function ServiceActions({ service, onStart, onStop, onRestart, onFreePort, startDisabled, dashboard = false }: Omit<ServiceRowProps, "selected" | "mode" | "onSelect"> & { dashboard?: boolean }) {
  const isRunning = service.state === "running";
  const isStarting = service.state === "starting";
  const isOccupied = service.state === "portInUse";
  return <div className={`az-service-row-actions${dashboard ? " az-service-row-actions-dashboard" : ""}`}>
    {isRunning && <><Button type="button" variant="secondary" size="sm" onClick={onStop}><Square />Stop</Button><Button type="button" variant="ghost" size="icon" className="az-inline-icon" onClick={onRestart} aria-label={`Restart ${SERVICE_LABELS[service.name]}`}><RotateCw /></Button></>}
    {isStarting && <Button type="button" variant="secondary" size="sm" disabled><LoaderCircle className="az-spin" />Starting</Button>}
    {!isRunning && !isStarting && !isOccupied && <Button type="button" variant="secondary" size="sm" onClick={onStart} disabled={startDisabled}><Play />Start</Button>}
    {isOccupied && <Button type="button" variant="destructive" size="sm" onClick={onFreePort}><Zap />Free port</Button>}
  </div>;
}

export function ServiceRow({ service, selected, mode = "popover", onSelect, onStart, onStop, onRestart, onFreePort, startDisabled = false }: ServiceRowProps) {
  const meta = SERVICE_META[service.name];
  if (mode === "dashboard") return <Card className={`az-service-row az-service-row-dashboard${selected ? " is-selected" : ""}`}>
    <div className="az-service-dashboard-top"><Button type="button" variant="ghost" className="az-service-select" onClick={onSelect} aria-pressed={selected}><ServiceGlyph name={service.name} /><span className="az-service-row-copy"><strong>{SERVICE_LABELS[service.name]}</strong><small>{meta.description}</small></span></Button><StatusPill state={service.state} compact /></div>
    <div className="az-service-dashboard-endpoint"><code>{service.host}:{service.port}</code><Button type="button" variant="ghost" size="icon" className="az-inline-icon" onClick={onSelect} aria-label={`Open ${SERVICE_LABELS[service.name]} details`}><ChevronRight /></Button></div>
    <div className="az-service-dashboard-meta"><span><TerminalSquare />{service.pid ? `PID ${service.pid}` : "No process"}</span><span><Activity />{formatUptime(service.uptimeSeconds)}</span><ServiceActions service={service} onStart={onStart} onStop={onStop} onRestart={onRestart} onFreePort={onFreePort} startDisabled={startDisabled} dashboard /></div>
  </Card>;
  return <Card className={`az-service-row az-service-row-popover${selected ? " is-selected" : ""}`}>
    <div className="az-service-row-main"><Button type="button" variant="ghost" className="az-service-select" onClick={onSelect} aria-pressed={selected}><ServiceGlyph name={service.name} /><span className="az-service-row-copy"><strong>{SERVICE_LABELS[service.name]}</strong><small>{meta.description}</small></span></Button><ServiceActions service={service} onStart={onStart} onStop={onStop} onRestart={onRestart} onFreePort={onFreePort} startDisabled={startDisabled} /></div>
    <div className={`az-service-state-strip az-state-${service.state}`}><code>{service.host}:{service.port}</code><span>{stateLabel(service.state)}</span><span>{service.pid ? `PID ${service.pid}` : "Awaiting process"}</span></div>
  </Card>;
}

export function ConfirmDialog({ title, body, confirmLabel, danger = true, onConfirm, onCancel }: { title: string; body: React.ReactNode; confirmLabel: string; danger?: boolean; onConfirm: () => void; onCancel: () => void }) {
  return <AlertDialog open onOpenChange={(open) => { if (!open) onCancel(); }}><AlertDialogContent className="az-dialog"><AlertDialogHeader><div className="az-dialog-kicker"><AlertCircle />Action requires confirmation</div><AlertDialogTitle>{title}</AlertDialogTitle><AlertDialogDescription>{body}</AlertDialogDescription></AlertDialogHeader><AlertDialogFooter><AlertDialogCancel onClick={onCancel}>Cancel</AlertDialogCancel><AlertDialogAction className={danger ? "az-dialog-danger" : ""} onClick={onConfirm}>{confirmLabel}</AlertDialogAction></AlertDialogFooter></AlertDialogContent></AlertDialog>;
}

function ServiceSkeleton() {
  return <>{SERVICE_ORDER.map((name) => <Card key={name} className="az-service-row az-service-skeleton"><span className="az-skeleton-icon" /><span className="az-skeleton-line az-skeleton-wide" /><span className="az-skeleton-line az-skeleton-short" /></Card>)}</>;
}

export function PopoverView({ model, onOpenDashboard, onHide }: { model: AzTrayModel; onOpenDashboard: () => void; onHide: () => void }) {
  const [confirm, setConfirm] = React.useState<{ title: string; body: React.ReactNode; label: string; action: () => Promise<void> } | null>(null);
  const [quitOpen, setQuitOpen] = React.useState(false);
  const services = model.services;
  const askStop = (service: ServiceSnapshot) => setConfirm({ title: `Stop ${SERVICE_LABELS[service.name]}?`, body: `AzTray will stop the app-owned process on ${service.host}:${service.port}.`, label: "Stop service", action: () => model.stop(service.name) });
  const askFree = (service: ServiceSnapshot) => setConfirm({ title: `Free port ${service.port}?`, body: service.portOwner ? `This requests termination of ${service.portOwner.name ?? "the process"} (PID ${service.portOwner.pid}) after identity revalidation.` : "AzTray will re-check the listener before taking action.", label: "Free port", action: () => model.freePort(service.name).then(() => undefined) });
  const ask = (title: string, body: React.ReactNode, label: string, action: () => Promise<void>) => setConfirm({ title, body, label, action });
  const runConfirm = () => { const action = confirm?.action; setConfirm(null); if (action) void action(); };
  return <ThemeFrame><main className="az-popover-window"><AppHeader model={model} title="AzTray" onRefresh={() => void model.refresh()} onClose={onHide} /><ScrollArea className="az-popover-scroll"><ActionErrorBanner model={model} /><section className="az-popover-overview"><div><span className="az-eyebrow">LOCAL EMULATOR</span><h1>{model.services ? `${model.runningCount}/3 online` : "Connecting"}</h1><p><span className="az-live-dot" />{model.lastEvent}</p></div><Button type="button" size="lg" onClick={() => void model.startAll()} disabled={!services || model.engine.state !== "ready"}><Play />Start all</Button></section><EngineBanner model={model} /><section className="az-popover-grid" aria-label="Azurite services">{services ? SERVICE_ORDER.map((name) => <ServiceRow key={name} service={services[name]} startDisabled={model.engine.state !== "ready"} onStart={() => void model.start(name)} onStop={() => askStop(services[name])} onRestart={() => ask(`Restart ${SERVICE_LABELS[name]}?`, "The service will briefly stop before it starts again.", "Restart service", () => model.restart(name))} onFreePort={() => askFree(services[name])} />) : <ServiceSkeleton />}<Card className="az-controller-cell"><div className="az-controller-cell-head"><span className="az-eyebrow">CONTROLLER</span><Badge variant="outline" className="az-activity-badge">{model.snapshot ? "SYNCED" : "WAITING"}</Badge></div><div className="az-controller-cell-copy"><strong>{model.services ? `${model.runningCount}/3 online` : "Waiting for status"}</strong><small>{model.snapshot?.config.host ?? "Controller heartbeat"}</small></div><div className="az-controller-cell-actions"><Button type="button" variant="secondary" onClick={() => ask("Stop all services?", "AzTray will stop every app-owned process. External processes remain untouched.", "Stop all", model.stopAll)} disabled={!services}><Square />Stop all</Button><Button type="button" variant="secondary" onClick={() => ask("Restart all services?", "All app-owned services will restart together.", "Restart all", model.restartAll)} disabled={!services || model.engine.state !== "ready"}><RotateCw />Restart</Button></div></Card></section><Card className="az-popover-activity-strip"><CardContent><div className="az-activity-icon"><Activity /></div><div><span className="az-eyebrow">RECENT ACTIVITY</span><strong>{model.lastEvent}</strong></div><Badge variant="outline" className="az-activity-badge">{model.snapshot ? "LIVE" : "WAITING"}</Badge></CardContent></Card><div className="az-popover-links"><Button type="button" variant="ghost" onClick={onOpenDashboard}><LayoutDashboard />Open dashboard<ArrowUpRight /></Button><Button type="button" variant="ghost" onClick={() => setQuitOpen(true)}><Power />Quit AzTray</Button></div></ScrollArea><footer className="az-popover-footer"><span><Cloud />{model.snapshot?.config.host ?? "127.0.0.1"}</span><span className="az-mono">{model.snapshot?.generatedAt ? `updated ${formatTimestamp(model.snapshot.generatedAt)}` : "connecting"}</span></footer>{confirm && <ConfirmDialog title={confirm.title} body={confirm.body} confirmLabel={confirm.label} onConfirm={runConfirm} onCancel={() => setConfirm(null)} />}{quitOpen && <QuitDialog model={model} onCancel={() => setQuitOpen(false)} onDone={onHide} />}</main></ThemeFrame>;
}

function ServiceOverviewCard({ service, selected, onSelect, onStart, onStop, onRestart, onFreePort, startDisabled }: ServiceRowProps) { return <ServiceRow service={service} mode="dashboard" selected={selected} onSelect={onSelect} onStart={onStart} onStop={onStop} onRestart={onRestart} onFreePort={onFreePort} startDisabled={startDisabled} />; }

export function DashboardView({ model, onHide }: { model: AzTrayModel; onHide: () => void }) {
  const [view, setView] = React.useState<"services" | "settings">("services");
  const [confirm, setConfirm] = React.useState<{ title: string; body: React.ReactNode; label: string; action: () => Promise<void> } | null>(null);
  const [quitOpen, setQuitOpen] = React.useState(false);
  const services = model.services;
  const selected = services?.[model.selectedService] ?? null;
  const ask = (title: string, body: React.ReactNode, label: string, action: () => Promise<void>) => setConfirm({ title, body, label, action });
  const runConfirm = () => { const action = confirm?.action; setConfirm(null); if (action) void action(); };
  React.useEffect(() => { let dispose: (() => void) | undefined; let alive = true; void aztrayIpc.subscribe("quit_requested", () => { if (alive) setQuitOpen(true); }).then((unlisten) => { if (alive) dispose = unlisten; else unlisten(); }); return () => { alive = false; dispose?.(); }; }, []);
  const state = overallState(model);
  return <ThemeFrame><main className="az-dashboard-window"><AppHeader model={model} title="AzTray" onRefresh={() => void model.refresh()} onClose={onHide} /><ActionErrorBanner model={model} /><div className="az-dashboard-body"><Tabs value={view} onValueChange={(value) => setView(value as typeof view)} orientation="vertical" className="az-dashboard-tabs"><aside className="az-rail"><div className="az-rail-brand"><HeaderMark /></div><TabsList className="az-rail-list"><TabsTrigger value="services" className="az-rail-trigger"><LayoutDashboard /><span>Overview</span></TabsTrigger><TabsTrigger value="settings" className="az-rail-trigger"><Settings2 /><span>Settings</span></TabsTrigger></TabsList><div className="az-rail-footer"><Button type="button" variant="ghost" size="icon" className="az-inline-icon" onClick={() => setQuitOpen(true)} aria-label="Quit AzTray"><Power /></Button></div></aside><TabsContent value="services" className="az-dashboard-content"><header className="az-dashboard-hero"><div><span className="az-eyebrow">CONTROLLER OVERVIEW</span><h1>Azurite workspace</h1><p>One place to start, inspect, and troubleshoot your local services.</p></div><div className="az-hero-actions"><StatusPill state={state} label={state === "running" ? "All services online" : `${model.runningCount}/3 online`} /><Button type="button" size="lg" onClick={() => void model.startAll()} disabled={!services || model.engine.state !== "ready"}><Play />Start all</Button></div></header><EngineBanner model={model} /><section className="az-overview-section"><div className="az-section-heading"><div><span className="az-eyebrow">SERVICE OVERVIEW</span><h2>Endpoints</h2></div><span className="az-section-note"><Server />{model.snapshot?.config.host ?? "127.0.0.1"}</span></div><div className="az-service-grid">{services ? SERVICE_ORDER.map((name) => <ServiceOverviewCard key={name} service={services[name]} selected={name === model.selectedService} startDisabled={model.engine.state !== "ready"} onSelect={() => model.selectService(name)} onStart={() => void model.start(name)} onStop={() => ask(`Stop ${SERVICE_LABELS[name]}?`, "The app-owned service process will exit.", "Stop service", () => model.stop(name))} onRestart={() => ask(`Restart ${SERVICE_LABELS[name]}?`, "The service will briefly stop before it starts again.", "Restart service", () => model.restart(name))} onFreePort={() => ask(`Free port ${services[name].port}?`, services[name].portOwner ? `Revalidate and release PID ${services[name].portOwner.pid}.` : "Re-check the listener before taking action.", "Free port", () => model.freePort(name).then(() => undefined))} />) : <ServiceSkeleton />}</div></section>{selected ? <ServiceDetail model={model} service={selected} ask={ask} /> : <EmptyDashboard model={model} />}<footer className="az-dashboard-footer"><span><Activity />{model.lastEvent}</span><Button type="button" variant="ghost" size="sm" onClick={() => setQuitOpen(true)}><Power />Quit AzTray</Button></footer></TabsContent><TabsContent value="settings" className="az-dashboard-content az-settings-content"><SettingsView model={model} /><footer className="az-dashboard-footer"><span><Settings2 />Settings apply to the next service start.</span><Button type="button" variant="ghost" size="sm" onClick={() => setQuitOpen(true)}><Power />Quit AzTray</Button></footer></TabsContent></Tabs></div>{confirm && <ConfirmDialog title={confirm.title} body={confirm.body} confirmLabel={confirm.label} onConfirm={runConfirm} onCancel={() => setConfirm(null)} />}{quitOpen && <QuitDialog model={model} onCancel={() => setQuitOpen(false)} onDone={onHide} />}</main></ThemeFrame>;
}

function ServiceDetail({ model, service, ask }: { model: AzTrayModel; service: ServiceSnapshot; ask: (title: string, body: React.ReactNode, label: string, action: () => Promise<void>) => void }) {
  const [logFilter, setLogFilter] = React.useState("");
  const [stream, setStream] = React.useState<"all" | "stdout" | "stderr" | "system">("all");
  const [logScope, setLogScope] = React.useState<"service" | "merged">("service");
  const [copied, setCopied] = React.useState(false);
  const [copyLogsState, setCopyLogsState] = React.useState<"idle" | "copied" | "failed">("idle");
  const allLogs = logScope === "merged" ? (model.snapshot?.mergedLogs ?? []) : (model.snapshot?.logs[service.name] ?? []);
  const logs = allLogs.filter((entry) => (stream === "all" || entry.stream === stream) && (!logFilter || entry.message.toLowerCase().includes(logFilter.toLowerCase())));
  const copyConnection = async () => { await model.copyConnectionString(service.name); setCopied(true); window.setTimeout(() => setCopied(false), 1400); };
  const copyLogs = async () => {
    const text = logs.map((entry) => `${entry.timestamp} [${entry.service}] [${entry.stream}] [${entry.level}] ${entry.message}`).join("\n");
    try {
      if (!navigator.clipboard) throw new Error("Clipboard unavailable");
      await navigator.clipboard.writeText(text);
      setCopyLogsState("copied");
      window.setTimeout(() => setCopyLogsState("idle"), 1400);
    } catch {
      setCopyLogsState("failed");
      window.setTimeout(() => setCopyLogsState("idle"), 2200);
    }
  };
  return <section className="az-detail-layout"><header className="az-detail-header"><div className="az-detail-heading"><ServiceGlyph name={service.name} /><div><span className="az-eyebrow">SELECTED SERVICE</span><h2>{SERVICE_LABELS[service.name]}</h2><code>{service.host}:{service.port}</code></div></div><StatusPill state={service.state} /><div className="az-detail-actions">{service.state === "running" && <Button type="button" variant="secondary" onClick={() => ask(`Stop ${SERVICE_LABELS[service.name]}?`, "The app-owned service process will exit.", "Stop service", () => model.stop(service.name))}><Square />Stop</Button>}{service.state === "portInUse" && <Button type="button" variant="destructive" onClick={() => ask(`Free port ${service.port}?`, service.portOwner ? `Revalidate and release ${service.portOwner.name ?? "the listener"} (PID ${service.portOwner.pid}).` : "Re-check the listener before taking action.", "Free port", () => model.freePort(service.name).then(() => undefined))}><Zap />Free port</Button>}{service.state !== "running" && service.state !== "starting" && service.state !== "portInUse" && <Button type="button" onClick={() => void model.start(service.name)} disabled={model.engine.state !== "ready"}><Play />Start</Button>}{service.state === "starting" && <Button type="button" variant="secondary" disabled><LoaderCircle className="az-spin" />Starting</Button>}<Button type="button" variant="secondary" onClick={() => ask(`Restart ${SERVICE_LABELS[service.name]}?`, "The service will briefly stop before it starts again.", "Restart service", () => model.restart(service.name))} disabled={model.engine.state !== "ready"}><RotateCw />Restart</Button></div></header>{service.error && <div className="az-detail-error"><AlertCircle />{service.error}</div>}<div className="az-detail-stats"><Stat label="PORT" value={String(service.port)} mono /><Stat label="PID" value={service.pid ? String(service.pid) : "—"} mono /><Stat label="UPTIME" value={formatUptime(service.uptimeSeconds)} mono /><Stat label="DATA DIRECTORY" value={model.snapshot?.config.dataDirectory || "Azurite default"} /></div><div className="az-workspace-grid"><div className="az-workspace-primary"><Card className="az-connection-card"><div className="az-connection-icon"><Clipboard /></div><div><span className="az-eyebrow">CONNECTION STRING</span><strong>Ready for your local client</strong><p>Copy a service-specific endpoint with the current host and port.</p></div><Button type="button" variant="secondary" onClick={() => void copyConnection()}>{copied ? <><Check />Copied</> : <><Copy />Copy string</>}</Button></Card><Card className="az-logs-card"><div className="az-logs-heading"><div><div className="az-logs-title"><TerminalSquare /><span className="az-eyebrow">LIVE ACTIVITY</span><Badge variant="outline" className="az-log-count">{logs.length} lines</Badge></div><span className="az-logs-subtitle">{logScope === "merged" ? "All services" : `${SERVICE_LABELS[service.name]} service`} · updates in real time</span></div><div className="az-logs-actions"><Tabs value={logScope} onValueChange={(value) => setLogScope(value as typeof logScope)}><TabsList className="az-segmented"><TabsTrigger value="service">Service</TabsTrigger><TabsTrigger value="merged">Merged</TabsTrigger></TabsList></Tabs><Input aria-label="Filter logs" placeholder="Filter" value={logFilter} onChange={(event) => setLogFilter(event.target.value)} className="az-log-filter" /><Select value={stream} onValueChange={(value) => setStream(value as typeof stream)}><SelectTrigger aria-label="Log stream" className="az-log-select"><SelectValue placeholder="All streams" /></SelectTrigger><SelectContent><SelectItem value="all">All streams</SelectItem><SelectItem value="stdout">stdout</SelectItem><SelectItem value="stderr">stderr</SelectItem><SelectItem value="system">system</SelectItem></SelectContent></Select><Button type="button" variant="outline" size="sm" className="az-copy-logs-button" onClick={() => void copyLogs()} aria-label="Copy visible logs">{copyLogsState === "copied" ? <Check /> : copyLogsState === "failed" ? <AlertCircle /> : <Copy />}<span>{copyLogsState === "copied" ? "Copied" : copyLogsState === "failed" ? "Copy failed" : "Copy logs"}</span></Button><Button type="button" variant="default" size="sm" onClick={() => void model.saveLogs(logScope === "service" ? service.name : undefined)}><FileDown />Export</Button></div></div><ScrollArea className="az-log-viewport" aria-live="polite">{logs.length ? logs.map((entry) => <div className={`az-log-line az-log-${entry.level}`} key={entry.id}><time>{formatTimestamp(entry.timestamp)}</time><span>{logScope === "merged" ? entry.service : entry.stream}</span><p>{entry.message}</p></div>) : <div className="az-empty-logs"><TerminalSquare /><strong>{service.state === "stopped" ? "Service output is waiting" : "No matching activity"}</strong><span>{service.state === "stopped" ? "Start the service to stream Azurite output." : "Try a different filter or stream."}</span></div>}</ScrollArea></Card></div><aside className="az-workspace-aside"><Card className="az-diagnostic-card"><CardHeader><CardTitle><CircleHelp />Diagnostics</CardTitle><CardDescription>Runtime details for this endpoint</CardDescription></CardHeader><CardContent><DiagnosticRow label="Process" value={service.processIdentity?.name ?? "Not app-owned"} /><DiagnosticRow label="Executable" value={service.processIdentity?.executablePath ?? "—"} mono /><DiagnosticRow label="Started" value={service.startedAt ? formatTimestamp(service.startedAt) : "—"} mono /><DiagnosticRow label="Exit code" value={service.exitCode === null ? "—" : String(service.exitCode)} mono /></CardContent></Card><Card className="az-data-card"><CardContent><div className="az-data-card-icon"><Database /></div><span className="az-eyebrow">DATA DIRECTORY</span><code>{model.snapshot?.config.dataDirectory || "Using Azurite default"}</code><small>AzTray never modifies stored service data.</small></CardContent></Card></aside></div></section>;
}

function Stat({ label, value, mono = false }: { label: string; value: string; mono?: boolean }) { return <div className="az-stat-cell"><span className="az-eyebrow">{label}</span><strong className={mono ? "az-mono" : ""}>{value}</strong></div>; }
function DiagnosticRow({ label, value, mono = false }: { label: string; value: string; mono?: boolean }) { return <div className="az-diagnostic-row"><span>{label}</span><strong className={mono ? "az-mono" : ""} title={value}>{value}</strong></div>; }
function EmptyDashboard({ model }: { model: AzTrayModel }) { return <Card className="az-empty-dashboard"><div className="az-empty-icon"><PackageOpen /></div><span className="az-eyebrow">READY WHEN YOU ARE</span><h2>Select a service to inspect it</h2><p>Start all services to bring your local storage endpoints online.</p><Button type="button" onClick={() => void model.startAll()} disabled={model.engine.state !== "ready"}><Play />Start all services</Button></Card>; }

function QuitDialog({ model, onCancel, onDone }: { model: AzTrayModel; onCancel: () => void; onDone: () => void }) {
  const [busy, setBusy] = React.useState(false);
  const choose = async (mode: "stop_and_quit" | "leave_running") => { setBusy(true); try { const didQuit = await model.quit(mode); if (didQuit) onDone(); else setBusy(false); } catch { setBusy(false); } };
  return <AlertDialog open onOpenChange={(open) => { if (!open && !busy) onCancel(); }}><AlertDialogContent className="az-dialog az-quit-dialog"><AlertDialogHeader><div className="az-dialog-kicker"><Power />Quit AzTray</div><AlertDialogTitle>What should happen to Azurite?</AlertDialogTitle><AlertDialogDescription>Choose whether app-owned services continue after the tray controller closes.</AlertDialogDescription></AlertDialogHeader><div className="az-quit-options"><Button type="button" variant="outline" className="az-quit-option" disabled={busy} onClick={() => void choose("stop_and_quit")}><Square /><span><strong>Stop &amp; quit</strong><small>Stop every service AzTray started, then exit.</small></span><ChevronRight /></Button><Button type="button" variant="outline" className="az-quit-option" disabled={busy} onClick={() => void choose("leave_running")}><Cloud /><span><strong>Leave running</strong><small>Keep Azurite available while AzTray exits.</small></span><ChevronRight /></Button></div><AlertDialogFooter><AlertDialogCancel onClick={onCancel} disabled={busy}>Cancel</AlertDialogCancel></AlertDialogFooter></AlertDialogContent></AlertDialog>;
}

function SettingsView({ model }: { model: AzTrayModel }) {
  const current = model.snapshot?.config;
  const [draft, setDraft] = React.useState<Config | null>(current ?? null);
  const syncedConfigRef = React.useRef<string | null>(null);
  React.useEffect(() => {
    if (!current) return;
    const serialized = JSON.stringify(current);
    setDraft((previous) => {
      if (previous === null || syncedConfigRef.current !== serialized) {
        syncedConfigRef.current = serialized;
        return current;
      }
      return previous;
    });
  }, [current]);
  if (!draft) return <div className="az-loading-panel"><LoaderCircle className="az-spin" />Loading settings…</div>;
  const updatePort = (service: ServiceName, value: string) => setDraft({ ...draft, ports: { ...draft.ports, [service]: Number(value) || 0 } });
  return <section className="az-settings-view"><header className="az-settings-heading"><div><span className="az-eyebrow">CONTROLLER SETTINGS</span><h1>Connection settings</h1><p>Configure how AzTray reaches the local Azurite engine. Changes apply to the next start.</p></div><Button type="button" size="lg" onClick={() => void model.saveConfig(draft)}><Check />Save settings</Button></header><EngineBanner model={model} /><Card className="az-settings-card"><CardHeader><CardTitle><Server />Endpoint</CardTitle><CardDescription>Host and service ports used by your local clients.</CardDescription></CardHeader><CardContent className="az-settings-fields"><label><span className="az-eyebrow">HOST</span><Input value={draft.host} onChange={(event) => setDraft({ ...draft, host: event.target.value })} /></label><div className="az-settings-grid">{SERVICE_ORDER.map((service) => <label key={service}><span className="az-eyebrow">{SERVICE_LABELS[service].toUpperCase()} PORT</span><Input inputMode="numeric" value={draft.ports[service]} onChange={(event) => updatePort(service, event.target.value)} /></label>)}</div><label><span className="az-eyebrow">DATA DIRECTORY</span><Input value={draft.dataDirectory} placeholder="Azurite default" onChange={(event) => setDraft({ ...draft, dataDirectory: event.target.value })} /><small>Stored Azurite data stays untouched by AzTray.</small></label></CardContent></Card><Card className="az-settings-card"><CardHeader><CardTitle><FolderOpen />Runtime overrides</CardTitle><CardDescription>Leave blank to use PATH and npm discovery.</CardDescription></CardHeader><CardContent className="az-settings-fields"><label><span className="az-eyebrow">AZURITE EXECUTABLE OVERRIDE</span><Input value={draft.executablePath ?? ""} placeholder="Use Node/npm discovery" onChange={(event) => setDraft({ ...draft, executablePath: event.target.value || null })} /></label><label><span className="az-eyebrow">NODE EXECUTABLE OVERRIDE</span><Input value={draft.nodePath ?? ""} placeholder="Use PATH discovery" onChange={(event) => setDraft({ ...draft, nodePath: event.target.value || null })} /></label></CardContent></Card></section>;
}

function ThemeFrame({ children }: { children: React.ReactNode }) { return <TooltipProvider>{children}</TooltipProvider>; }
