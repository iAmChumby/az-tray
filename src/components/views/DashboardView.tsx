"use client";

import * as React from "react";
import { Play, Plus, Power, ScrollText, Settings2, Square, Waypoints } from "lucide-react";
import { Button } from "@/src/components/ui/button";
import { Skeleton } from "@/src/components/ui/skeleton";
import { AppHeader, AppNotices } from "@/src/components/common/AppChrome";
import { useConfirm } from "@/src/components/common/ConfirmDialog";
import { EmptyState } from "@/src/components/common/EmptyState";
import { QuitDialog } from "@/src/components/common/QuitDialog";
import { CreateInstanceDialog } from "@/src/components/instances/CreateInstanceDialog";
import { InstanceDetail } from "@/src/components/instances/InstanceDetail";
import { InstanceList } from "@/src/components/instances/InstanceList";
import { LogsView } from "@/src/components/logs/LogsView";
import { McpIndicator } from "@/src/components/mcp/McpIndicator";
import { McpPanel } from "@/src/components/mcp/McpPanel";
import { SettingsView } from "@/src/components/settings/SettingsView";
import type { AzTrayModel } from "@/src/hooks/useAzTray";
import { aztrayIpc } from "@/src/lib/ipc";
import { cn } from "@/src/lib/utils";

type View = "instance" | "logs" | "mcp" | "settings";

const NAV: { view: Exclude<View, "instance">; label: string; icon: React.ReactNode }[] = [
  { view: "logs", label: "Logs", icon: <ScrollText /> },
  { view: "mcp", label: "MCP server", icon: <Waypoints /> },
  { view: "settings", label: "Settings", icon: <Settings2 /> },
];

export function DashboardView({ model, onHide }: { model: AzTrayModel; onHide: () => void }) {
  const [view, setView] = React.useState<View>("instance");
  const [createOpen, setCreateOpen] = React.useState(false);
  const [quitOpen, setQuitOpen] = React.useState(false);
  const [confirmDialog, ask] = useConfirm();
  const { selected } = model;

  // The tray menu's Quit asks the dashboard to show the quit choices.
  React.useEffect(() => {
    let alive = true;
    let dispose: (() => void) | undefined;
    void aztrayIpc.subscribe("quit_requested", () => { if (alive) setQuitOpen(true); }).then((unlisten) => {
      if (alive) dispose = unlisten; else unlisten();
    });
    return () => { alive = false; dispose?.(); };
  }, []);

  const ready = model.engine.state === "ready";
  const anyActive = model.instances.some((item) => item.state !== "stopped");
  const allBusy = model.isPending("all");

  return (
    <main className="flex h-dvh min-h-0 flex-col bg-background">
      <AppHeader model={model} onClose={onHide} />
      <div className="grid min-h-0 flex-1 grid-cols-[16.5rem_minmax(0,1fr)]">
        <aside className="flex min-h-0 flex-col gap-4 overflow-y-auto border-r border-border bg-sidebar p-3">
          <InstanceList
            model={model}
            activeId={view === "instance" ? model.selectedInstanceId : null}
            onSelect={(id) => { model.selectInstance(id); setView("instance"); }}
            onCreate={() => setCreateOpen(true)}
          />
          <Button type="button" variant="outline" className="w-full justify-start" onClick={() => setCreateOpen(true)}><Plus />New instance</Button>
          <div className="grid grid-cols-2 gap-2">
            <Button type="button" variant="secondary" size="sm" disabled={!ready || allBusy || !model.instances.length} onClick={() => void model.startAll()}><Play />Start all</Button>
            <Button
              type="button"
              variant="secondary"
              size="sm"
              disabled={allBusy || !anyActive}
              onClick={() => ask({ title: "Stop every instance?", body: "All Azurite services AzTray started will stop. Data on disk is kept.", confirmLabel: "Stop all", action: model.stopAll })}
            >
              <Square />Stop all
            </Button>
          </div>

          <nav aria-label="Sections" className="mt-auto grid gap-0.5 border-t border-border pt-3">
            {NAV.map((item) => (
              <button
                key={item.view}
                type="button"
                aria-current={view === item.view ? "page" : undefined}
                onClick={() => setView(item.view)}
                className={cn("flex h-9 items-center gap-2.5 rounded-lg px-2.5 text-sm outline-none transition-colors focus-visible:ring-3 focus-visible:ring-ring/50 [&_svg]:size-4 [&_svg]:text-muted-foreground", view === item.view ? "bg-accent font-medium text-accent-foreground" : "text-muted-foreground hover:bg-accent/60 hover:text-foreground")}
              >
                {item.icon}{item.label}
              </button>
            ))}
            <div className="mt-2 flex items-center justify-between gap-1 border-t border-border pt-2">
              <McpIndicator model={model} onOpen={() => setView("mcp")} />
              <Button type="button" variant="ghost" size="icon-sm" aria-label="Quit AzTray" title="Quit AzTray" onClick={() => setQuitOpen(true)}><Power /></Button>
            </div>
          </nav>
        </aside>

        <div className="min-h-0 overflow-y-auto" data-testid="dashboard-main">
          <AppNotices model={model} className="mx-auto max-w-5xl px-6 pt-5" />
          {view === "instance" && (
            model.loading ? (
              <div className="mx-auto grid max-w-5xl gap-4 p-6"><Skeleton className="h-10 w-64" /><Skeleton className="h-8 w-80" /><Skeleton className="h-56 rounded-xl" /></div>
            ) : selected ? (
              <InstanceDetail model={model} instance={selected} />
            ) : (
              <EmptyState icon={<Plus />} title="No instances yet" className="h-full" action={<Button onClick={() => setCreateOpen(true)}><Plus />Create an instance</Button>}>
                An instance is a Blob, Queue, and Table set with its own ports and data folder.
              </EmptyState>
            )
          )}
          {view === "logs" && <LogsView model={model} />}
          {view === "mcp" && <McpPanel model={model} />}
          {view === "settings" && <SettingsView model={model} onOpenMcp={() => setView("mcp")} />}
        </div>
      </div>
      {confirmDialog}
      <CreateInstanceDialog model={model} open={createOpen} onOpenChange={setCreateOpen} />
      <QuitDialog model={model} open={quitOpen} onOpenChange={setQuitOpen} onDone={onHide} />
    </main>
  );
}
