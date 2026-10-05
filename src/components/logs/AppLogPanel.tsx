"use client";

import * as React from "react";
import { FileText, RefreshCw } from "lucide-react";
import { toast } from "sonner";
import { Button } from "@/src/components/ui/button";
import { CopyButton } from "@/src/components/common/CopyButton";
import { EmptyState } from "@/src/components/common/EmptyState";
import { actionErrorMessage } from "@/src/lib/actionError";
import { aztrayIpc } from "@/src/lib/ipc";
import { cn } from "@/src/lib/utils";

function lineTone(line: string) {
  if (/\bERROR\b/.test(line)) return "text-status-broken";
  if (/\bWARN\b/.test(line)) return "text-status-starting";
  return "text-foreground";
}

/** Tail of AzTray's own diagnostic log file (aztray.log): MCP lifecycle, config, engine events. */
export function AppLogPanel({ logPath, limit = 300, heightClass = "h-[28rem]" }: { logPath?: string; limit?: number; heightClass?: string }) {
  const [lines, setLines] = React.useState<string[] | null>(null);
  const [loading, setLoading] = React.useState(false);
  const scrollRef = React.useRef<HTMLDivElement>(null);

  const load = React.useCallback(async () => {
    setLoading(true);
    try {
      setLines(await aztrayIpc.getAppLog(limit));
    } catch (cause) {
      toast.error("Unable to read the AzTray log", { description: actionErrorMessage(cause, "The log file could not be read") });
    } finally {
      setLoading(false);
    }
  }, [limit]);

  React.useEffect(() => { void load(); }, [load]);
  React.useEffect(() => {
    const el = scrollRef.current;
    if (el) el.scrollTop = el.scrollHeight;
  }, [lines]);

  return (
    <div className="overflow-hidden rounded-xl border border-border bg-card">
      <div className="flex flex-wrap items-center gap-2 border-b border-border p-2.5">
        <div className="min-w-0 flex-1">
          <p className="text-sm font-medium">AzTray app log</p>
          {logPath && <code className="block truncate text-xs text-muted-foreground" title={logPath}>{logPath}</code>}
        </div>
        <CopyButton variant="labeled" buttonVariant="outline" text={() => (lines ?? []).join("\n")} label="app log" />
        <Button type="button" variant="outline" size="sm" onClick={() => void load()} disabled={loading}>
          <RefreshCw className={cn(loading && "animate-spin")} />Refresh
        </Button>
      </div>
      <div ref={scrollRef} className={cn("az-scroll overflow-auto bg-muted/60", heightClass)} role="log" aria-label="AzTray app log" tabIndex={0}>
        {lines && lines.length ? (
          <ol className="min-w-max py-1 font-mono text-[12.5px] leading-relaxed">
            {lines.map((line, index) => <li key={index} className={cn("px-3 py-px whitespace-pre hover:bg-foreground/5", lineTone(line))}>{line}</li>)}
          </ol>
        ) : (
          <EmptyState icon={<FileText />} title={lines ? "The app log is empty" : "Loading the app log"} className="h-full">
            {lines ? "AzTray writes MCP, config, and engine events here as they happen." : undefined}
          </EmptyState>
        )}
      </div>
    </div>
  );
}
