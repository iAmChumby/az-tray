"use client";

import * as React from "react";
import { FolderOpen } from "lucide-react";
import { Button } from "@/src/components/ui/button";
import { Input } from "@/src/components/ui/input";
import { Label } from "@/src/components/ui/label";
import { Switch } from "@/src/components/ui/switch";
import { canPickFolder, pickDirectory } from "@/src/lib/dialogs";
import { SERVICE_LABELS, SERVICE_NAMES, type InstanceConfig, type ServiceName } from "@/src/lib/types";
import type { PortProblems } from "@/src/lib/validation";
import { cn } from "@/src/lib/utils";
import { toast } from "sonner";

/** Editable form state. Ports are strings so partial typing stays possible. */
export type InstanceFormValues = {
  name: string;
  host: string;
  ports: Record<ServiceName, string>;
  dataDirectory: string;
  loose: boolean;
  skipApiVersionCheck: boolean;
};

export function valuesFromConfig(config: InstanceConfig): InstanceFormValues {
  return {
    name: config.name,
    host: config.host,
    ports: { blob: String(config.ports.blob), queue: String(config.ports.queue), table: String(config.ports.table) },
    dataDirectory: config.dataDirectory,
    loose: config.loose,
    skipApiVersionCheck: config.skipApiVersionCheck,
  };
}

export function portsToNumbers(ports: Record<ServiceName, string>): Record<ServiceName, number> {
  return { blob: Number(ports.blob), queue: Number(ports.queue), table: Number(ports.table) };
}

type Props = {
  idPrefix: string;
  values: InstanceFormValues;
  onChange: (next: InstanceFormValues) => void;
  nameError?: string | null;
  portErrors?: PortProblems;
  /** Shown under the name field, e.g. the generated id. */
  idHint?: string;
  /** Locks host / ports / data directory / flags (name stays editable). */
  structureLocked?: boolean;
  onDataDirectoryEdited?: () => void;
};

export function InstanceFields({ idPrefix, values, onChange, nameError, portErrors = {}, idHint, structureLocked = false, onDataDirectoryEdited }: Props) {
  const set = <K extends keyof InstanceFormValues>(key: K, value: InstanceFormValues[K]) => onChange({ ...values, [key]: value });
  const browse = async () => {
    try {
      const dir = await pickDirectory(values.dataDirectory);
      if (dir) { set("dataDirectory", dir); onDataDirectoryEdited?.(); }
    } catch (cause) {
      toast.error("Unable to open the folder picker", { description: cause instanceof Error ? cause.message : undefined });
    }
  };
  return (
    <div className="grid gap-4">
      <div className="grid gap-1.5">
        <Label htmlFor={`${idPrefix}-name`}>Name</Label>
        <Input id={`${idPrefix}-name`} value={values.name} onChange={(event) => set("name", event.target.value)} aria-invalid={!!nameError} aria-describedby={`${idPrefix}-name-hint`} autoComplete="off" maxLength={60} />
        <p id={`${idPrefix}-name-hint`} className={cn("text-xs", nameError ? "text-status-broken" : "text-muted-foreground")}>
          {nameError ?? (idHint ? <>Instance id <code className="rounded bg-foreground/8 px-1">{idHint}</code> is used by agents and the CLI and cannot change later.</> : "Shown in the sidebar and the tray.")}
        </p>
      </div>

      <fieldset className="grid gap-1.5" disabled={structureLocked}>
        <legend className="mb-1.5 text-sm font-medium">Ports</legend>
        <div className="grid grid-cols-3 gap-3">
          {SERVICE_NAMES.map((name) => (
            <div key={name} className="grid gap-1">
              <Label htmlFor={`${idPrefix}-port-${name}`} className="text-xs font-normal text-muted-foreground">{SERVICE_LABELS[name]}</Label>
              <Input
                id={`${idPrefix}-port-${name}`}
                inputMode="numeric"
                className="font-mono tabular-nums"
                value={values.ports[name]}
                aria-invalid={!!portErrors[name]}
                aria-describedby={portErrors[name] ? `${idPrefix}-port-${name}-error` : undefined}
                onChange={(event) => set("ports", { ...values.ports, [name]: event.target.value.replace(/[^0-9]/g, "") })}
              />
            </div>
          ))}
        </div>
        {SERVICE_NAMES.map((name) => portErrors[name] && (
          <p key={name} id={`${idPrefix}-port-${name}-error`} className="text-xs text-status-broken" role="alert">{SERVICE_LABELS[name]} port: {portErrors[name]}</p>
        ))}
        {!SERVICE_NAMES.some((name) => portErrors[name]) && !structureLocked && <p className="text-xs text-muted-foreground">Suggested from the next free block. Edit any port to choose your own.</p>}
      </fieldset>

      <div className="grid gap-1.5">
        <Label htmlFor={`${idPrefix}-data`}>Data folder</Label>
        <div className="flex gap-2">
          <Input id={`${idPrefix}-data`} className="font-mono text-[13px]" value={values.dataDirectory} disabled={structureLocked} onChange={(event) => { set("dataDirectory", event.target.value); onDataDirectoryEdited?.(); }} spellCheck={false} />
          <Button type="button" variant="outline" disabled={structureLocked} onClick={() => void browse()} title={canPickFolder() ? "Choose a folder" : "The folder picker is available in the desktop app"}>
            <FolderOpen />Browse
          </Button>
        </div>
        <p className="text-xs text-muted-foreground">AzTray creates this folder when the instance starts and never reads or deletes what is inside.</p>
      </div>

      <details className="group rounded-lg border border-border">
        <summary className="cursor-pointer rounded-lg px-3 py-2 text-sm font-medium outline-none focus-visible:ring-3 focus-visible:ring-ring/50">Advanced</summary>
        <div className="grid gap-4 border-t border-border p-3">
          <div className="grid gap-1.5">
            <Label htmlFor={`${idPrefix}-host`}>Host</Label>
            <Input id={`${idPrefix}-host`} className="font-mono text-[13px]" value={values.host} disabled={structureLocked} onChange={(event) => set("host", event.target.value)} />
            <p className="text-xs text-muted-foreground">Use 127.0.0.1 for this machine only, or 0.0.0.0 to accept connections from the network.</p>
          </div>
          <div className="flex items-start justify-between gap-4">
            <div className="grid gap-0.5">
              <Label htmlFor={`${idPrefix}-loose`}>Loose mode</Label>
              <p className="text-xs text-muted-foreground">Ignore unsupported headers and parameters instead of failing the request.</p>
            </div>
            <Switch id={`${idPrefix}-loose`} checked={values.loose} disabled={structureLocked} onCheckedChange={(checked) => set("loose", checked)} />
          </div>
          <div className="flex items-start justify-between gap-4">
            <div className="grid gap-0.5">
              <Label htmlFor={`${idPrefix}-skip`}>Skip API version check</Label>
              <p className="text-xs text-muted-foreground">Accept SDK versions newer than this Azurite release.</p>
            </div>
            <Switch id={`${idPrefix}-skip`} checked={values.skipApiVersionCheck} disabled={structureLocked} onCheckedChange={(checked) => set("skipApiVersionCheck", checked)} />
          </div>
        </div>
      </details>
    </div>
  );
}
