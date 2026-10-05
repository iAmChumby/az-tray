"use client";

import * as React from "react";
import { ArrowUpRight, Copy, LayoutDashboard, Play, Plus, Power, Square } from "lucide-react";
import { Button } from "@/src/components/ui/button";
import { Skeleton } from "@/src/components/ui/skeleton";
import { AppHeader, AppNotices } from "@/src/components/common/AppChrome";
import { useConfirm } from "@/src/components/common/ConfirmDialog";
import { CopyButton } from "@/src/components/common/CopyButton";
import { PortStrip } from "@/src/components/common/PortStrip";
import { QuitDialog } from "@/src/components/common/QuitDialog";
import { StatusBadge, StatusDot } from "@/src/components/common/Status";
import { CreateInstanceDialog } from "@/src/components/instances/CreateInstanceDialog";
import { InstanceActions } from "@/src/components/instances/InstanceActions";
import { ServiceRow } from "@/src/components/instances/ServiceRow";
import { freePortRequest } from "@/src/components/instances/freePort";
import { McpIndicator } from "@/src/components/mcp/McpIndicator";
import type { AzTrayModel } from "@/src/hooks/useAzTray";
import { pluralize } from "@/src/lib/format";
import { instanceStateLabel, instanceTone } from "@/src/lib/status";
import { SERVICE_NAMES } from "@/src/lib/types";
import { cn } from "@/src/lib/utils";

/** The compact tray popover: switch instance, control its three services, see MCP at a glance. */
export function PopoverView({ model, onOpenDashboard, onHide }: { model: AzTrayModel; onOpenDashboard: () => void; onHide: () => void }) {
  const [createOpen, setCreateOpen] = React.useState(false);
  const [quitOpen, setQuitOpen] = React.useState(false);
  const [confirmDialog, ask] = useConfirm();
  const { selected, instances } = model;
  const ready = model.engine.state === "ready";
  const anyActive = instances.some((item) => item.state !== "stopped");
  const allBusy = model.isPending("all");
  const many = instances.length > 1;

  return (
    <main className="flex h-dvh min-h-0 flex-col bg-background">
      <AppHeader model={model} onClose={onHide} />
      <div className="az-scroll min-h-0 flex-1 overflow-y-auto">
        <div className="grid gap-3 p-3">
          <AppNotices model={model} />

          <div className="flex items-center justify-between gap-2">
            <p className="text-sm text-muted-foreground" aria-live="polite">
              {model.loading ? "Connecting" : many ? <><strong className="font-semibold text-foreground">{model.runningInstanceCount}</strong> of {pluralize(instances.length, "instance")} running</> : "Local Azurite"}
            </p>
            <div className="flex gap-1.5">
              <Button type="button" size="sm" variant="secondary" disabled={!ready || allBusy || !instances.length} onClick={() => void model.startAll()}><Play />{many ? "Start all" : "Start"}</Button>
              <Button type="button" size="sm" variant="secondary" disabled={allBusy || !anyActive} onClick={() => ask({ title: many ? "Stop every instance?" : "Stop Azurite?", body: "All Azurite services AzTray started will stop. Data on disk is kept.", confirmLabel: "Stop", action: model.stopAll })}><Square />{many ? "Stop all" : "Stop"}</Button>
            </div>
          </div>

          <div role="tablist" aria-label="Instances" className="az-scroll -mx-3 flex gap-1.5 overflow-x-auto px-3 pb-1">
            {model.loading && [0, 1].map((key) => <Skeleton key={key} className="h-8 w-28 shrink-0 rounded-full" />)}
            {instances.map((instance) => {
              const active = instance.config.id === selected?.config.id;
              return (
                <button
                  key={instance.config.id}
                  type="button"
                  role="tab"
                  aria-selected={active}
                  onClick={() => model.selectInstance(instance.config.id)}
                  className={cn("inline-flex h-8 shrink-0 items-center gap-2 rounded-full border px-3 text-sm outline-none transition-colors focus-visible:ring-3 focus-visible:ring-ring/50", active ? "border-primary/60 bg-primary/12 font-medium text-foreground" : "border-border text-muted-foreground hover:bg-accent/60 hover:text-foreground")}
                >
                  <StatusDot tone={instanceTone(instance.state)} />
                  {instance.config.name}
                  <span className="sr-only">{instanceStateLabel(instance.state)}</span>
                </button>
              );
            })}
            <button type="button" onClick={() => setCreateOpen(true)} aria-label="New instance" title="New instance" className="inline-flex size-8 shrink-0 items-center justify-center rounded-full border border-dashed border-border-strong text-muted-foreground outline-none hover:bg-accent/60 hover:text-foreground focus-visible:ring-3 focus-visible:ring-ring/50"><Plus className="size-4" /></button>
          </div>

          {model.loading ? (
            <div className="grid gap-2"><Skeleton className="h-24 rounded-xl" />{SERVICE_NAMES.map((name) => <Skeleton key={name} className="h-16 rounded-lg" />)}</div>
          ) : selected ? (
            <>
              <section aria-label={`${selected.config.name} controls`} className="grid gap-3 rounded-xl border border-border bg-card p-3">
                <div className="flex items-center justify-between gap-2">
                  <h1 className="truncate text-base font-semibold tracking-tight">{selected.config.name}</h1>
                  <StatusBadge tone={instanceTone(selected.state)} label={instanceStateLabel(selected.state)} />
                </div>
                <PortStrip instance={selected} />
                <div className="flex flex-wrap items-center gap-1.5">
                  <InstanceActions model={model} instance={selected} ask={ask} />
                  <CopyButton variant="labeled" buttonVariant="outline" className="ml-auto" text={selected.connection.connectionString} label="connection string">Connection string</CopyButton>
                </div>
              </section>
              <ul className="grid gap-2" aria-label="Services">
                {SERVICE_NAMES.map((name) => (
                  <ServiceRow key={name} model={model} service={selected.services[name]} variant="compact" confirmFreePort={(service) => ask(freePortRequest(model, service))} />
                ))}
              </ul>
            </>
          ) : null}
        </div>
      </div>

      <footer className="flex shrink-0 items-center justify-between gap-2 border-t border-border bg-card/80 px-2 py-1.5">
        <McpIndicator model={model} onOpen={onOpenDashboard} />
        <div className="flex items-center gap-0.5">
          <Button type="button" variant="ghost" size="sm" onClick={onOpenDashboard}><LayoutDashboard />Dashboard<ArrowUpRight className="opacity-60" /></Button>
          <Button type="button" variant="ghost" size="icon-sm" aria-label="Quit AzTray" title="Quit AzTray" onClick={() => setQuitOpen(true)}><Power /></Button>
        </div>
      </footer>
      {confirmDialog}
      <CreateInstanceDialog model={model} open={createOpen} onOpenChange={setCreateOpen} />
      <QuitDialog model={model} open={quitOpen} onOpenChange={setQuitOpen} onDone={onHide} />
    </main>
  );
}
