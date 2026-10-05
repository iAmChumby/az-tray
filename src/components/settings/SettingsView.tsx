"use client";

import * as React from "react";
import { AlertTriangle, ArrowRight, Check, Monitor, Moon, Save, Sun } from "lucide-react";
import { Button } from "@/src/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/src/components/ui/card";
import { Input } from "@/src/components/ui/input";
import { Label } from "@/src/components/ui/label";
import { CopyButton } from "@/src/components/common/CopyButton";
import { StatusBadge } from "@/src/components/common/Status";
import { AppLogPanel } from "@/src/components/logs/AppLogPanel";
import type { AzTrayModel } from "@/src/hooks/useAzTray";
import { useTheme, type ThemeMode } from "@/src/hooks/useTheme";
import { mcpLabel, mcpPhase, mcpTone } from "@/src/lib/status";
import { cn } from "@/src/lib/utils";

const THEMES: { mode: ThemeMode; label: string; icon: React.ReactNode }[] = [
  { mode: "light", label: "Light", icon: <Sun /> },
  { mode: "dark", label: "Dark", icon: <Moon /> },
  { mode: "system", label: "System", icon: <Monitor /> },
];

function PathRow({ label, value }: { label: string; value: string }) {
  return (
    <div className="flex items-center gap-2">
      <div className="grid min-w-0 flex-1 gap-0.5">
        <span className="text-xs text-muted-foreground">{label}</span>
        <code className="truncate text-[13px]" title={value}>{value}</code>
      </div>
      <CopyButton text={value} label={label.toLowerCase()} />
    </div>
  );
}

function AppearanceCard() {
  const { mode, setMode } = useTheme();
  const [mounted, setMounted] = React.useState(false);
  React.useEffect(() => setMounted(true), []);
  return (
    <Card>
      <CardHeader>
        <CardTitle>Appearance</CardTitle>
        <CardDescription>Applies to the tray popover and the dashboard.</CardDescription>
      </CardHeader>
      <CardContent>
        <div role="radiogroup" aria-label="Theme" className="inline-flex rounded-lg bg-muted p-0.5">
          {THEMES.map((item) => {
            const active = mounted && mode === item.mode;
            return (
              <button
                key={item.mode}
                type="button"
                role="radio"
                aria-checked={active}
                onClick={() => setMode(item.mode)}
                className={cn("inline-flex h-8 items-center gap-1.5 rounded-md px-3 text-sm outline-none transition-colors focus-visible:ring-3 focus-visible:ring-ring/50 [&_svg]:size-4", active ? "bg-card text-foreground shadow-sm ring-1 ring-border" : "text-muted-foreground hover:text-foreground")}
              >
                {item.icon}{item.label}
              </button>
            );
          })}
        </div>
      </CardContent>
    </Card>
  );
}

function EngineCard({ model }: { model: AzTrayModel }) {
  const config = model.config;
  const [paths, setPaths] = React.useState({ executablePath: "", nodePath: "" });
  React.useEffect(() => {
    if (config) setPaths({ executablePath: config.executablePath ?? "", nodePath: config.nodePath ?? "" });
  }, [config?.executablePath, config?.nodePath]); // eslint-disable-line react-hooks/exhaustive-deps
  if (!config) return null;
  const dirty = (config.executablePath ?? "") !== paths.executablePath || (config.nodePath ?? "") !== paths.nodePath;
  const anyRunning = model.instances.some((item) => item.state !== "stopped");
  const save = async (event: React.FormEvent) => {
    event.preventDefault();
    await model.saveSettings({ executablePath: paths.executablePath.trim() || null, nodePath: paths.nodePath.trim() || null, mcp: config.mcp });
  };
  const { engine } = model;
  return (
    <Card>
      <CardHeader>
        <CardTitle>Azurite engine</CardTitle>
        <CardDescription>Shared by every instance. Leave the overrides blank to use PATH and npm discovery.</CardDescription>
      </CardHeader>
      <CardContent className="grid gap-4">
        <dl className="grid gap-3 sm:grid-cols-2">
          <div className="grid gap-0.5"><dt className="text-xs text-muted-foreground">Azurite</dt><dd className="text-sm">{engine.azuriteVersion ? `v${engine.azuriteVersion}` : "Not found"}</dd></div>
          <div className="grid gap-0.5"><dt className="text-xs text-muted-foreground">Node.js</dt><dd className="text-sm">{engine.nodeVersion ?? "Not found"}</dd></div>
        </dl>
        <form onSubmit={save} className="grid gap-4">
          <div className="grid gap-1.5">
            <Label htmlFor="exe-path">Azurite executable override</Label>
            <Input id="exe-path" className="font-mono text-[13px]" value={paths.executablePath} placeholder={engine.azuritePath ?? "Use Node and npm discovery"} disabled={anyRunning} onChange={(event) => setPaths({ ...paths, executablePath: event.target.value })} spellCheck={false} />
          </div>
          <div className="grid gap-1.5">
            <Label htmlFor="node-path">Node.js executable override</Label>
            <Input id="node-path" className="font-mono text-[13px]" value={paths.nodePath} placeholder={engine.nodePath ?? "Use PATH discovery"} disabled={anyRunning} onChange={(event) => setPaths({ ...paths, nodePath: event.target.value })} spellCheck={false} />
          </div>
          <div className="flex items-center justify-between gap-3">
            <p className="text-xs text-muted-foreground">{anyRunning ? "Stop every instance to change these paths." : "Applies the next time an instance starts."}</p>
            <Button type="submit" disabled={!dirty || anyRunning || model.isPending("settings")}><Save />Save paths</Button>
          </div>
        </form>
      </CardContent>
    </Card>
  );
}

export function SettingsView({ model, onOpenMcp }: { model: AzTrayModel; onOpenMcp: () => void }) {
  const phase = mcpPhase(model.mcp);
  const app = model.app;
  const hasMcp = !!app?.features.includes("mcp");
  return (
    <div className="mx-auto grid w-full max-w-3xl gap-5 p-6">
      <header className="grid gap-1">
        <h1 className="text-xl font-semibold tracking-tight">Settings</h1>
        <p className="text-sm text-muted-foreground">Global options. Ports and data folders live in each instance&apos;s own settings.</p>
      </header>

      <AppearanceCard />
      <EngineCard model={model} />

      <Card>
        <CardHeader>
          <CardTitle>Local MCP server</CardTitle>
          <CardDescription>Port, fallback, and the agent configuration snippet.</CardDescription>
        </CardHeader>
        <CardContent className="flex items-center justify-between gap-3">
          <div className="flex items-center gap-3">
            {model.mcp && <StatusBadge tone={mcpTone(phase)} label={mcpLabel(phase)} />}
            <code className="truncate text-sm text-muted-foreground">{model.mcp?.url}</code>
          </div>
          <Button type="button" variant="outline" onClick={onOpenMcp}>Open MCP page<ArrowRight /></Button>
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle>About</CardTitle>
          <CardDescription>Check the version when an agent cannot reach the MCP server: older builds do not include it.</CardDescription>
        </CardHeader>
        <CardContent className="grid gap-4">
          <div className="flex flex-wrap items-center gap-2">
            <strong className="text-base font-semibold">AzTray {app ? `v${app.version}` : ""}</strong>
            {app?.features.map((feature) => (
              <span key={feature} className="inline-flex h-5 items-center gap-1 rounded-full bg-foreground/8 px-2 font-mono text-[11px] text-muted-foreground"><Check className="size-3 text-status-running" aria-hidden="true" />{feature}</span>
            ))}
          </div>
          {app && !hasMcp && (
            <p className="flex items-start gap-2 rounded-lg border border-status-starting/30 bg-status-starting/10 px-3 py-2 text-sm"><AlertTriangle className="mt-0.5 size-4 shrink-0 text-status-starting" aria-hidden="true" />This build does not include the MCP server. Install the latest release.</p>
          )}
          {app && (
            <div className="grid gap-3">
              <PathRow label="Config file" value={app.configPath} />
              <PathRow label="Diagnostic log" value={app.logPath} />
            </div>
          )}
        </CardContent>
      </Card>

      <section className="grid gap-2" aria-labelledby="diagnostics-heading">
        <div className="grid gap-0.5">
          <h2 id="diagnostics-heading" className="text-base font-medium">Diagnostics</h2>
          <p className="text-sm text-muted-foreground">The tail of AzTray&apos;s own log: MCP start-up, port fallbacks, and config migration.</p>
        </div>
        <AppLogPanel logPath={app?.logPath} limit={200} heightClass="h-72" />
      </section>
    </div>
  );
}
