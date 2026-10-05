"use client";

import * as React from "react";
import { LoaderCircle, RotateCw } from "lucide-react";
import { Button } from "@/src/components/ui/button";
import { StatusDot } from "@/src/components/common/Status";
import type { AzTrayModel } from "@/src/hooks/useAzTray";
import { mcpLabel, mcpPhase, mcpTone } from "@/src/lib/status";
import { cn } from "@/src/lib/utils";

/** Compact MCP status: dot, "MCP :port", and Retry when failed. Used in the popover footer and dashboard sidebar. */
export function McpIndicator({ model, onOpen, className }: { model: AzTrayModel; onOpen?: () => void; className?: string }) {
  const status = model.mcp;
  const phase = mcpPhase(status);
  const tone = mcpTone(phase);
  const retrying = model.isPending("mcp");
  const detail =
    phase === "running" ? `:${status?.port}${status?.fallbackUsed ? " (fallback)" : ""}`
      : phase === "failed" ? "unreachable"
        : phase === "disabled" ? "off"
          : "starting";
  const title = status ? `${mcpLabel(phase)}: ${status.url}${status.error ? `. ${status.error}` : ""}` : "MCP status unknown";
  const body = (
    <>
      <StatusDot tone={retrying ? "starting" : tone} className="size-2" />
      <span className="font-medium">MCP</span>
      <span className={cn("font-mono text-xs", phase === "failed" ? "text-status-broken" : "text-muted-foreground")}>{detail}</span>
    </>
  );
  return (
    <div className={cn("flex items-center gap-1.5 text-sm", className)}>
      {onOpen ? (
        <button type="button" onClick={onOpen} title={title} className="flex items-center gap-1.5 rounded-md px-1.5 py-1 outline-none hover:bg-accent focus-visible:ring-3 focus-visible:ring-ring/50">{body}</button>
      ) : (
        <span className="flex items-center gap-1.5 px-1.5 py-1" title={title}>{body}</span>
      )}
      {phase === "failed" && (
        <Button type="button" size="xs" variant="secondary" onClick={() => void model.restartMcp()} disabled={retrying}>
          {retrying ? <LoaderCircle className="animate-spin" /> : <RotateCw />}Retry
        </Button>
      )}
    </div>
  );
}
