"use client";

import * as React from "react";
import { AlertTriangle, Monitor, Moon, RefreshCw, Sun, X } from "lucide-react";
import { Button } from "@/src/components/ui/button";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/src/components/ui/tooltip";
import type { AzTrayModel } from "@/src/hooks/useAzTray";
import { useTheme } from "@/src/hooks/useTheme";
import { pluralize } from "@/src/lib/format";
import { cn } from "@/src/lib/utils";

/** Three bars of different length: Blob, Queue, Table ports on one rail. */
export function BrandMark({ className }: { className?: string }) {
  return (
    <span aria-hidden="true" className={cn("inline-flex size-7 shrink-0 flex-col justify-center gap-[3px] rounded-lg bg-primary px-[7px]", className)}>
      <span className="h-[3px] w-full rounded-full bg-primary-foreground" />
      <span className="h-[3px] w-3/5 rounded-full bg-primary-foreground/80" />
      <span className="h-[3px] w-4/5 rounded-full bg-primary-foreground/60" />
    </span>
  );
}

export function ThemeToggle() {
  const { mode, theme, toggle, setMode } = useTheme();
  const [mounted, setMounted] = React.useState(false);
  React.useEffect(() => setMounted(true), []);
  const label = mounted ? (theme === "dark" ? "Switch to light theme" : "Switch to dark theme") : "Change theme";
  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <Button
          type="button"
          variant="ghost"
          size="icon-sm"
          aria-label={label}
          onClick={toggle}
          onContextMenu={(event) => { event.preventDefault(); setMode("system"); }}
        >
          {!mounted ? <Monitor /> : theme === "dark" ? <Sun /> : <Moon />}
        </Button>
      </TooltipTrigger>
      <TooltipContent>{label}{mounted && mode === "system" ? " (following system)" : ""}</TooltipContent>
    </Tooltip>
  );
}

function IconAction({ label, onClick, children }: { label: string; onClick: () => void; children: React.ReactNode }) {
  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <Button type="button" variant="ghost" size="icon-sm" aria-label={label} onClick={onClick}>{children}</Button>
      </TooltipTrigger>
      <TooltipContent>{label}</TooltipContent>
    </Tooltip>
  );
}

/** Frameless-window title bar. The empty area drags the window; buttons opt out. */
export function AppHeader({ model, onClose, className }: { model: AzTrayModel; onClose?: () => void; className?: string }) {
  const total = model.instances.length;
  const summary = model.loading ? "Connecting" : total === 0 ? "No instances" : `${model.runningInstanceCount} of ${pluralize(total, "instance")} running`;
  return (
    <header data-tauri-drag-region className={cn("flex h-12 shrink-0 items-center gap-3 border-b border-border bg-card/80 px-3 select-none", className)}>
      <div data-tauri-drag-region className="pointer-events-none flex items-center gap-2.5">
        <BrandMark />
        <div className="flex items-baseline gap-2">
          <strong className="text-sm font-semibold tracking-tight">AzTray</strong>
          {model.app && <span className="font-mono text-[11px] text-faint">v{model.app.version}</span>}
        </div>
      </div>
      <span data-tauri-drag-region className="pointer-events-none ml-1 hidden truncate text-xs text-muted-foreground sm:inline">{summary}</span>
      <div data-tauri-drag-region className="flex-1 self-stretch" />
      <div className="flex items-center gap-0.5">
        <ThemeToggle />
        <IconAction label="Refresh status" onClick={() => void model.refresh()}><RefreshCw /></IconAction>
        {onClose && <IconAction label="Hide to tray" onClick={onClose}><X /></IconAction>}
      </div>
    </header>
  );
}

/** Everything that needs attention before normal use: unreachable controller, stale build, config problems, missing engine. */
export function AppNotices({ model, className }: { model: AzTrayModel; className?: string }) {
  const notices: { key: string; title: string; body: string; hint?: string }[] = [];
  if (model.loadError) notices.push({ key: "load", title: "AzTray is not responding", body: model.loadError });
  if (model.app && !model.app.features.includes("mcp")) {
    notices.push({ key: "stale", title: "This build has no MCP server", body: `AzTray v${model.app.version} predates the local MCP endpoint, so agents cannot connect.`, hint: "Install the latest release from GitHub, then reopen AzTray." });
  }
  if (model.app?.configError) notices.push({ key: "config", title: "Configuration notice", body: model.app.configError });
  if (!model.loading && model.engine.state !== "ready") {
    const title = model.engine.state === "missingNode" ? "Node.js was not found" : model.engine.state === "missingAzurite" ? "Azurite was not found" : "Azurite needs attention";
    notices.push({ key: "engine", title, body: model.engine.message ?? "Set an executable path in Settings to enable the controls.", hint: model.engine.installHint ?? undefined });
  }
  if (!notices.length) return null;
  return (
    <div className={cn("grid gap-2", className)} role="status">
      {notices.map((notice) => (
        <div key={notice.key} className="flex gap-2.5 rounded-lg border border-status-starting/30 bg-status-starting/10 px-3 py-2.5 text-sm">
          <AlertTriangle className="mt-0.5 size-4 shrink-0 text-status-starting" aria-hidden="true" />
          <div className="grid min-w-0 gap-0.5">
            <strong className="font-medium">{notice.title}</strong>
            <span className="text-muted-foreground">{notice.body}</span>
            {notice.hint && <code className="mt-1 w-fit max-w-full truncate rounded bg-foreground/8 px-1.5 py-0.5 text-xs">{notice.hint}</code>}
          </div>
        </div>
      ))}
    </div>
  );
}
