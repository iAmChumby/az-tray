"use client";

import * as React from "react";
import { Save } from "lucide-react";
import { Button } from "@/src/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/src/components/ui/card";
import { Input } from "@/src/components/ui/input";
import { Label } from "@/src/components/ui/label";
import { Switch } from "@/src/components/ui/switch";
import type { AzTrayModel } from "@/src/hooks/useAzTray";
import { SERVICE_NAMES, type McpConfig } from "@/src/lib/types";

/** Enabled / port / fallback. Saving writes settings and restarts the listener. */
export function McpConfigCard({ model }: { model: AzTrayModel }) {
  const saved = model.config?.mcp;
  const [draft, setDraft] = React.useState<{ enabled: boolean; port: string; portFallback: boolean } | null>(null);
  const serialized = JSON.stringify(saved);
  React.useEffect(() => {
    if (saved) setDraft({ enabled: saved.enabled, port: String(saved.port), portFallback: saved.portFallback });
  }, [serialized]); // eslint-disable-line react-hooks/exhaustive-deps

  if (!saved || !draft || !model.config) return null;
  const port = Number(draft.port);
  let portError: string | null = null;
  if (!Number.isInteger(port) || port < 1024 || port > 65535) portError = "Use a port from 1024 to 65535";
  else {
    const clash = model.instances.find((item) => SERVICE_NAMES.some((name) => item.config.ports[name] >= port && item.config.ports[name] <= port + 9));
    if (clash) portError = `The range ${port}-${port + 9} overlaps ports used by "${clash.config.name}"`;
  }
  const next: McpConfig = { enabled: draft.enabled, port, portFallback: draft.portFallback };
  const dirty = JSON.stringify(next) !== JSON.stringify(saved);
  const submit = async (event: React.FormEvent) => {
    event.preventDefault();
    if (portError || !dirty) return;
    await model.saveSettings({ executablePath: model.config?.executablePath ?? null, nodePath: model.config?.nodePath ?? null, mcp: next });
  };

  return (
    <Card>
      <CardHeader>
        <CardTitle>Listener settings</CardTitle>
        <CardDescription>Saving restarts the MCP listener. Agents connected to the old address need the new URL if the port changes.</CardDescription>
      </CardHeader>
      <CardContent>
        <form onSubmit={submit} className="grid gap-4">
          <div className="flex items-start justify-between gap-4">
            <div className="grid gap-0.5">
              <Label htmlFor="mcp-enabled">Enable the MCP server</Label>
              <p className="text-xs text-muted-foreground">Lets coding agents list, create, and control Azurite instances.</p>
            </div>
            <Switch id="mcp-enabled" checked={draft.enabled} onCheckedChange={(enabled) => setDraft({ ...draft, enabled })} />
          </div>
          <div className="grid gap-1.5 sm:max-w-48">
            <Label htmlFor="mcp-port">Preferred port</Label>
            <Input id="mcp-port" inputMode="numeric" className="font-mono tabular-nums" value={draft.port} disabled={!draft.enabled} aria-invalid={!!portError} onChange={(event) => setDraft({ ...draft, port: event.target.value.replace(/[^0-9]/g, "") })} />
            {portError && <p className="text-xs text-status-broken" role="alert">{portError}</p>}
          </div>
          <div className="flex items-start justify-between gap-4">
            <div className="grid gap-0.5">
              <Label htmlFor="mcp-fallback">Try the next ports when it is busy</Label>
              <p className="text-xs text-muted-foreground">Uses the first free port from {Number.isFinite(port) ? `${port} to ${port + 9}` : "the preferred port up to +9"}. Turn off to keep a fixed URL.</p>
            </div>
            <Switch id="mcp-fallback" checked={draft.portFallback} disabled={!draft.enabled} onCheckedChange={(portFallback) => setDraft({ ...draft, portFallback })} />
          </div>
          <div className="flex justify-end gap-2">
            <Button type="button" variant="ghost" disabled={!dirty} onClick={() => setDraft({ enabled: saved.enabled, port: String(saved.port), portFallback: saved.portFallback })}>Discard changes</Button>
            <Button type="submit" disabled={!dirty || !!portError || model.isPending("settings") || model.isPending("mcp")}><Save />Save and restart MCP</Button>
          </div>
        </form>
      </CardContent>
    </Card>
  );
}
