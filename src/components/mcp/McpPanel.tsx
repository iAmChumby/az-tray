"use client";

import * as React from "react";
import { AlertTriangle, LoaderCircle, RotateCw } from "lucide-react";
import { Button } from "@/src/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/src/components/ui/card";
import { Skeleton } from "@/src/components/ui/skeleton";
import { CodeBlock } from "@/src/components/common/CodeBlock";
import { CopyButton } from "@/src/components/common/CopyButton";
import { StatusBadge } from "@/src/components/common/Status";
import { McpConfigCard } from "@/src/components/mcp/McpConfigCard";
import type { AzTrayModel } from "@/src/hooks/useAzTray";
import { claudeCliCommand, claudeConfigSnippet, formatDateTime } from "@/src/lib/format";
import { getMockMcpMode, isMockRuntime, setMockMcpMode, type MockMcpMode } from "@/src/lib/mockBackend";
import { mcpLabel, mcpPhase, mcpTone } from "@/src/lib/status";

function Fact({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div className="grid min-w-0 gap-0.5">
      <dt className="text-xs text-muted-foreground">{label}</dt>
      <dd className="min-w-0 truncate text-sm">{children}</dd>
    </div>
  );
}

function MockControls() {
  const [mode, setMode] = React.useState<MockMcpMode>(getMockMcpMode());
  const modes: MockMcpMode[] = ["running", "fallback", "failed", "disabled"];
  return (
    <div className="flex flex-wrap items-center gap-2 rounded-lg border border-dashed border-border-strong px-3 py-2 text-sm">
      <span className="text-muted-foreground">Browser preview: simulate</span>
      {modes.map((item) => (
        <Button key={item} type="button" size="xs" variant={mode === item ? "default" : "outline"} onClick={() => { setMockMcpMode(item); setMode(item); }}>{item}</Button>
      ))}
    </div>
  );
}

/** Dashboard "MCP" page: live status, retry, copyable agent config, and listener settings. */
export function McpPanel({ model }: { model: AzTrayModel }) {
  const status = model.mcp;
  const phase = mcpPhase(status);
  const retrying = model.isPending("mcp");
  const [mock, setMock] = React.useState(false);
  React.useEffect(() => setMock(isMockRuntime()), []);

  return (
    <div className="mx-auto grid w-full max-w-5xl gap-5 p-6">
      <header className="flex flex-wrap items-start justify-between gap-4">
        <div className="grid gap-1">
          <div className="flex items-center gap-3">
            <h1 className="text-xl font-semibold tracking-tight">Local MCP server</h1>
            {status && <StatusBadge tone={mcpTone(phase)} label={mcpLabel(phase)} />}
          </div>
          <p className="max-w-xl text-sm text-muted-foreground">Coding agents connect here to create Azurite instances, read connection strings, and check logs.</p>
        </div>
        <Button type="button" variant={phase === "failed" ? "default" : "secondary"} onClick={() => void model.restartMcp()} disabled={retrying || !status?.enabled}>
          {retrying ? <LoaderCircle className="animate-spin" /> : <RotateCw />}
          {phase === "failed" ? "Retry now" : "Restart MCP"}
        </Button>
      </header>

      {!status ? (
        <Skeleton className="h-40 rounded-xl" />
      ) : (
        <>
          {phase === "failed" && (
            <div className="flex gap-2.5 rounded-lg border border-status-broken/30 bg-status-broken/10 px-3 py-2.5 text-sm" role="alert">
              <AlertTriangle className="mt-0.5 size-4 shrink-0 text-status-broken" aria-hidden="true" />
              <div className="grid min-w-0 gap-1">
                <strong className="font-medium">MCP could not start on port {status.requestedPort}</strong>
                <span className="break-words text-muted-foreground">{status.error}</span>
                <span className="text-muted-foreground">AzTray retries on its own. {status.fallbackUsed ? "" : "Turn on port fallback below to use a nearby port, or free the port and press Retry."}</span>
              </div>
            </div>
          )}
          {status.running && status.fallbackUsed && (
            <div className="flex gap-2.5 rounded-lg border border-status-starting/30 bg-status-starting/10 px-3 py-2.5 text-sm" role="status">
              <AlertTriangle className="mt-0.5 size-4 shrink-0 text-status-starting" aria-hidden="true" />
              <div className="grid gap-0.5">
                <strong className="font-medium">Using fallback port {status.port}</strong>
                <span className="text-muted-foreground">Port {status.requestedPort} was busy. Agents configured with the old address must use the URL below.</span>
              </div>
            </div>
          )}

          <Card>
            <CardHeader>
              <CardTitle>Endpoint</CardTitle>
              <CardDescription>{status.running ? "Reachable on this computer only." : status.enabled ? "The address AzTray is trying to use." : "The server is turned off. Enable it in listener settings."}</CardDescription>
            </CardHeader>
            <CardContent className="grid gap-4">
              <div className="flex items-center gap-1 rounded-lg border border-border bg-muted px-3 py-2">
                <code className="min-w-0 flex-1 truncate text-sm" data-testid="mcp-url">{status.url}</code>
                <CopyButton variant="labeled" buttonVariant="outline" text={status.url} label="MCP URL">Copy URL</CopyButton>
              </div>
              <dl className="grid gap-4 sm:grid-cols-4">
                <Fact label="Port">{status.port}{status.fallbackUsed ? <span className="text-muted-foreground"> (wanted {status.requestedPort})</span> : null}</Fact>
                <Fact label="Attempts">{status.attempts}</Fact>
                <Fact label="Running since">{status.running ? formatDateTime(status.startedAt) : "-"}</Fact>
                <Fact label="Last event"><span title={status.lastEvent ?? undefined}>{status.lastEvent ?? "-"}</span></Fact>
              </dl>
            </CardContent>
          </Card>

          <Card>
            <CardHeader>
              <CardTitle>Connect Claude Code</CardTitle>
              <CardDescription>Add this entry to the <code>mcpServers</code> section of your Claude Code settings, or run the command.</CardDescription>
            </CardHeader>
            <CardContent className="grid gap-3">
              <CodeBlock code={claudeConfigSnippet(status)} label="Claude Code config" caption="mcpServers entry" />
              <CodeBlock code={claudeCliCommand(status)} label="claude mcp add command" caption="Or from a terminal" />
            </CardContent>
          </Card>
        </>
      )}

      <McpConfigCard model={model} />
      {mock && <MockControls />}
    </div>
  );
}
