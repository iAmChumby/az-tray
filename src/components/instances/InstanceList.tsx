"use client";

import * as React from "react";
import { Plus } from "lucide-react";
import { Button } from "@/src/components/ui/button";
import { Skeleton } from "@/src/components/ui/skeleton";
import { StatusDot } from "@/src/components/common/Status";
import type { AzTrayModel } from "@/src/hooks/useAzTray";
import { instanceStateLabel, instanceTone } from "@/src/lib/status";
import { cn } from "@/src/lib/utils";

/** Sidebar list of instances; each row is a native button so Tab and Enter work. */
export function InstanceList({ model, activeId, onSelect, onCreate }: { model: AzTrayModel; activeId: string | null; onSelect: (id: string) => void; onCreate: () => void }) {
  return (
    <section aria-label="Instances" className="grid gap-1.5">
      <div className="flex items-center justify-between px-2">
        <h2 className="text-xs font-medium text-muted-foreground">Instances</h2>
        <Button type="button" variant="ghost" size="icon-xs" aria-label="New instance" onClick={onCreate}><Plus /></Button>
      </div>
      {model.loading ? (
        <div className="grid gap-1.5">{[0, 1, 2].map((key) => <Skeleton key={key} className="h-12 rounded-lg" />)}</div>
      ) : (
        <ul className="grid gap-1">
          {model.instances.map((instance) => {
            const active = instance.config.id === activeId;
            const tone = instanceTone(instance.state);
            const { blob, table } = instance.config.ports;
            return (
              <li key={instance.config.id}>
                <button
                  type="button"
                  onClick={() => onSelect(instance.config.id)}
                  aria-current={active ? "true" : undefined}
                  className={cn(
                    "flex w-full items-center gap-2.5 rounded-lg px-2.5 py-2 text-left outline-none transition-colors focus-visible:ring-3 focus-visible:ring-ring/50",
                    active ? "bg-accent text-accent-foreground" : "hover:bg-accent/60",
                  )}
                >
                  <StatusDot tone={tone} className="size-2.5" />
                  <span className="grid min-w-0 flex-1">
                    <span className="truncate text-sm font-medium">{instance.config.name}</span>
                    <span className="truncate font-mono text-[11px] tabular-nums text-muted-foreground">{blob} to {table}</span>
                  </span>
                  <span className="sr-only">{instanceStateLabel(instance.state)}</span>
                </button>
              </li>
            );
          })}
        </ul>
      )}
    </section>
  );
}
