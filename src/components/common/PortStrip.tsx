import * as React from "react";
import { StatusDot } from "@/src/components/common/Status";
import { cn } from "@/src/lib/utils";
import { SERVICE_LABELS, SERVICE_NAMES, type InstanceSnapshot } from "@/src/lib/types";
import { serviceTone } from "@/src/lib/status";

/**
 * The signature element: an instance's three ports as one segmented strip, each
 * segment lit with its service's live state. Reads like a patch bay.
 */
export function PortStrip({ instance, className, showLabels = true }: { instance: InstanceSnapshot; className?: string; showLabels?: boolean }) {
  return (
    <ul className={cn("flex overflow-hidden rounded-md border border-border bg-muted text-xs", className)} aria-label="Ports">
      {SERVICE_NAMES.map((name, index) => {
        const service = instance.services[name];
        return (
          <li key={name} className={cn("flex min-w-0 flex-1 items-center gap-1.5 px-2 py-1", index > 0 && "border-l border-border")} title={`${SERVICE_LABELS[name]} ${service.host}:${service.port}`}>
            <StatusDot tone={serviceTone(service.state)} className="size-1.5" />
            {showLabels && <span className="text-muted-foreground">{SERVICE_LABELS[name][0]}</span>}
            <span className="font-mono tabular-nums">{service.port}</span>
          </li>
        );
      })}
    </ul>
  );
}
