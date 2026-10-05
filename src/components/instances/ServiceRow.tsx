"use client";

import * as React from "react";
import { LoaderCircle, Play, RotateCw, Square, Zap } from "lucide-react";
import { Button } from "@/src/components/ui/button";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/src/components/ui/tooltip";
import { CopyButton } from "@/src/components/common/CopyButton";
import { SERVICE_DESCRIPTIONS, ServiceGlyph } from "@/src/components/common/ServiceGlyph";
import { StatusBadge } from "@/src/components/common/Status";
import type { AzTrayModel } from "@/src/hooks/useAzTray";
import { formatUptime } from "@/src/lib/format";
import { serviceStateLabel, serviceTone } from "@/src/lib/status";
import { SERVICE_LABELS, type ServiceSnapshot } from "@/src/lib/types";
import { cn } from "@/src/lib/utils";

type Props = {
  model: AzTrayModel;
  service: ServiceSnapshot;
  variant: "compact" | "full";
  confirmFreePort: (service: ServiceSnapshot) => void;
};

function ServiceActions({ model, service, confirmFreePort }: Omit<Props, "variant">) {
  const label = SERVICE_LABELS[service.name];
  const pending = model.isPending(`${service.instanceId}:${service.name}`) || model.isPending(service.instanceId) || model.isPending("all");
  const ready = model.engine.state === "ready";
  const id = service.instanceId;
  if (service.state === "portInUse") {
    return <Button type="button" size="sm" variant="destructive" onClick={() => confirmFreePort(service)} disabled={pending}><Zap />Free port</Button>;
  }
  if (service.state === "starting" || pending) {
    return <Button type="button" size="sm" variant="secondary" disabled><LoaderCircle className="animate-spin" />Working</Button>;
  }
  if (service.state === "running") {
    return (
      <>
        <Button type="button" size="sm" variant="secondary" onClick={() => void model.stopService(id, service.name)} aria-label={`Stop ${label}`}><Square />Stop</Button>
        <Tooltip>
          <TooltipTrigger asChild>
            <Button type="button" size="icon-sm" variant="ghost" onClick={() => void model.restartService(id, service.name)} aria-label={`Restart ${label}`}><RotateCw /></Button>
          </TooltipTrigger>
          <TooltipContent>Restart {label}</TooltipContent>
        </Tooltip>
      </>
    );
  }
  return <Button type="button" size="sm" variant="secondary" onClick={() => void model.startService(id, service.name)} disabled={!ready} aria-label={`Start ${label}`}><Play />Start</Button>;
}

export function ServiceRow(props: Props) {
  const { service, variant } = props;
  const tone = serviceTone(service.state);
  const label = SERVICE_LABELS[service.name];
  const endpoint = `${service.host}:${service.port}`;

  if (variant === "compact") {
    return (
      <li className="grid gap-1.5 rounded-lg border border-border bg-card px-3 py-2.5">
        <div className="flex items-center gap-2.5">
          <ServiceGlyph name={service.name} />
          <div className="min-w-0 flex-1">
            <div className="flex items-center gap-2">
              <strong className="text-sm font-medium">{label}</strong>
              <StatusBadge tone={tone} label={serviceStateLabel(service.state)} className="h-5 px-2 text-[11px]" />
            </div>
            <code className="block truncate text-xs text-muted-foreground">{endpoint}</code>
          </div>
          <div className="flex shrink-0 items-center gap-1"><ServiceActions {...props} /></div>
        </div>
        {service.error && <p className="text-xs text-status-broken">{service.error}</p>}
      </li>
    );
  }

  return (
    <li className="grid gap-2 px-4 py-3">
      <div className="grid grid-cols-[minmax(0,1.1fr)_minmax(0,1.4fr)_auto_auto] items-center gap-4 max-lg:grid-cols-[minmax(0,1fr)_auto]">
        <div className="flex min-w-0 items-center gap-3">
          <ServiceGlyph name={service.name} className="size-9" />
          <div className="min-w-0">
            <strong className="block text-sm font-medium">{label}</strong>
            <span className="block truncate text-xs text-muted-foreground">{SERVICE_DESCRIPTIONS[service.name]}</span>
          </div>
        </div>
        <div className="flex min-w-0 items-center gap-1 max-lg:order-last max-lg:col-span-2">
          <code className="truncate text-[13px]">{endpoint}</code>
          <CopyButton text={`http://${endpoint}`} label={`${label} URL`} />
        </div>
        <div className={cn("flex min-w-[8.5rem] items-center justify-end gap-3 text-xs text-muted-foreground max-lg:hidden")}>
          <span className="font-mono tabular-nums">{service.pid ? `PID ${service.pid}` : "no process"}</span>
          <span className="font-mono tabular-nums">{formatUptime(service.uptimeSeconds)}</span>
        </div>
        <div className="flex items-center justify-end gap-1.5">
          <StatusBadge tone={tone} label={serviceStateLabel(service.state)} />
          <ServiceActions {...props} />
        </div>
      </div>
      {service.error && <p className="rounded-md bg-status-broken/10 px-2.5 py-1.5 text-xs text-status-broken">{service.error}</p>}
    </li>
  );
}
