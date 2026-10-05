"use client";

import * as React from "react";
import { Download, Terminal, Trash2 } from "lucide-react";
import { Button } from "@/src/components/ui/button";
import { Input } from "@/src/components/ui/input";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/src/components/ui/select";
import { CopyButton } from "@/src/components/common/CopyButton";
import { EmptyState } from "@/src/components/common/EmptyState";
import { SERVICE_TEXT } from "@/src/components/common/ServiceGlyph";
import type { AzTrayModel } from "@/src/hooks/useAzTray";
import { formatClock } from "@/src/lib/format";
import { SERVICE_LABELS, SERVICE_NAMES, type LogEntry, type LogStream, type ServiceName } from "@/src/lib/types";
import { cn } from "@/src/lib/utils";

const RENDER_LIMIT = 600;
const LEVEL_TEXT: Record<LogEntry["level"], string> = { info: "text-foreground", warn: "text-status-starting", error: "text-status-broken" };

type Props = {
  model: AzTrayModel;
  /** When set, logs are locked to this instance and the instance filter is hidden. */
  fixedInstanceId?: string;
  heightClass?: string;
};

/** Filterable Azurite output across one instance or all of them. */
export function LogViewer({ model, fixedInstanceId, heightClass = "h-[28rem]" }: Props) {
  const [instanceFilter, setInstanceFilter] = React.useState<string>("all");
  const [service, setService] = React.useState<ServiceName | "all">("all");
  const [stream, setStream] = React.useState<LogStream | "all">("all");
  const [text, setText] = React.useState("");
  const scrollRef = React.useRef<HTMLDivElement>(null);
  const pinned = React.useRef(true);

  const activeInstance = fixedInstanceId ?? instanceFilter;
  const multi = activeInstance === "all";

  const all = React.useMemo(() => {
    const sources = model.instances.filter((item) => activeInstance === "all" || item.config.id === activeInstance);
    const entries = sources.flatMap((item) => (service === "all" ? item.mergedLogs ?? [] : item.logs?.[service] ?? []));
    if (sources.length > 1) entries.sort((a, b) => a.timestamp.localeCompare(b.timestamp) || a.sequence - b.sequence);
    return entries;
  }, [model.instances, activeInstance, service]);

  const needle = text.trim().toLowerCase();
  const visible = React.useMemo(
    () => all.filter((entry) => (stream === "all" || entry.stream === stream) && (!needle || entry.message.toLowerCase().includes(needle))),
    [all, stream, needle],
  );
  const shown = visible.slice(-RENDER_LIMIT);
  const names = React.useMemo(() => new Map(model.instances.map((item) => [item.config.id, item.config.name])), [model.instances]);

  React.useEffect(() => {
    const el = scrollRef.current;
    if (el && pinned.current) el.scrollTop = el.scrollHeight;
  }, [shown.length, activeInstance, service, stream, needle]);

  const asText = () => visible.map((entry) => `${entry.timestamp} [${names.get(entry.instanceId) ?? entry.instanceId}] [${entry.service}] [${entry.stream}] [${entry.level}] ${entry.message}`).join("\n");
  const targetInstance = multi ? undefined : activeInstance;
  const targetService = service === "all" ? undefined : service;

  return (
    <div className="overflow-hidden rounded-xl border border-border bg-card">
      <div className="flex flex-wrap items-center gap-2 border-b border-border p-2.5">
        {!fixedInstanceId && (
          <Select value={instanceFilter} onValueChange={setInstanceFilter}>
            <SelectTrigger size="sm" aria-label="Instance" className="w-40"><SelectValue /></SelectTrigger>
            <SelectContent>
              <SelectItem value="all">All instances</SelectItem>
              {model.instances.map((item) => <SelectItem key={item.config.id} value={item.config.id}>{item.config.name}</SelectItem>)}
            </SelectContent>
          </Select>
        )}
        <Select value={service} onValueChange={(value) => setService(value as ServiceName | "all")}>
          <SelectTrigger size="sm" aria-label="Service" className="w-28"><SelectValue /></SelectTrigger>
          <SelectContent>
            <SelectItem value="all">All services</SelectItem>
            {SERVICE_NAMES.map((name) => <SelectItem key={name} value={name}>{SERVICE_LABELS[name]}</SelectItem>)}
          </SelectContent>
        </Select>
        <Select value={stream} onValueChange={(value) => setStream(value as LogStream | "all")}>
          <SelectTrigger size="sm" aria-label="Stream" className="w-28"><SelectValue /></SelectTrigger>
          <SelectContent>
            <SelectItem value="all">All streams</SelectItem>
            <SelectItem value="stdout">stdout</SelectItem>
            <SelectItem value="stderr">stderr</SelectItem>
            <SelectItem value="system">system</SelectItem>
          </SelectContent>
        </Select>
        <Input aria-label="Filter log text" placeholder="Filter text" value={text} onChange={(event) => setText(event.target.value)} className="h-7 min-w-32 flex-1 text-[13px]" />
        <span className="px-1 text-xs text-muted-foreground tabular-nums" aria-live="polite">{visible.length} lines</span>
        <CopyButton variant="labeled" buttonVariant="outline" text={asText} label="visible logs" />
        <Button type="button" variant="outline" size="sm" onClick={() => void model.saveLogs(targetInstance, targetService)} disabled={multi}>
          <Download />Export
        </Button>
        <Button type="button" variant="ghost" size="sm" onClick={() => void model.clearLogs(targetInstance, targetService)} disabled={multi || !all.length}>
          <Trash2 />Clear
        </Button>
      </div>
      <div
        ref={scrollRef}
        onScroll={(event) => { const el = event.currentTarget; pinned.current = el.scrollHeight - el.scrollTop - el.clientHeight < 24; }}
        className={cn("az-scroll overflow-auto bg-muted/60", heightClass)}
        role="log"
        aria-label="Azurite output"
        tabIndex={0}
      >
        {shown.length ? (
          <ol className="min-w-max py-1 font-mono text-[12.5px] leading-relaxed">
            {shown.map((entry) => (
              <li key={entry.id} className="flex gap-3 px-3 py-px hover:bg-foreground/5">
                <time className="shrink-0 text-faint tabular-nums">{formatClock(entry.timestamp)}</time>
                {multi && <span className="w-28 shrink-0 truncate text-muted-foreground">{names.get(entry.instanceId) ?? entry.instanceId}</span>}
                <span className={cn("w-12 shrink-0", SERVICE_TEXT[entry.service])}>{entry.service}</span>
                <span className={cn("whitespace-pre", LEVEL_TEXT[entry.level])}>{entry.message}</span>
              </li>
            ))}
          </ol>
        ) : (
          <EmptyState icon={<Terminal />} title={all.length ? "No lines match these filters" : "No output yet"} className="h-full">
            {all.length ? "Clear the filter text or pick another stream." : "Start an instance and Azurite output streams here as it happens."}
          </EmptyState>
        )}
      </div>
    </div>
  );
}
