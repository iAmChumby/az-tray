"use client";

import * as React from "react";
import { Database, Network } from "lucide-react";
import { Card } from "@/src/components/ui/card";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/src/components/ui/tabs";
import { CopyButton } from "@/src/components/common/CopyButton";
import { useConfirm } from "@/src/components/common/ConfirmDialog";
import { PortStrip } from "@/src/components/common/PortStrip";
import { StatusBadge } from "@/src/components/common/Status";
import { ConnectionPanel } from "@/src/components/instances/ConnectionPanel";
import { InstanceActions } from "@/src/components/instances/InstanceActions";
import { InstanceSettingsTab } from "@/src/components/instances/InstanceSettingsTab";
import { ServiceRow } from "@/src/components/instances/ServiceRow";
import { freePortRequest } from "@/src/components/instances/freePort";
import { LogViewer } from "@/src/components/logs/LogViewer";
import type { AzTrayModel } from "@/src/hooks/useAzTray";
import { instanceStateLabel, instanceTone } from "@/src/lib/status";
import { SERVICE_NAMES, type InstanceSnapshot } from "@/src/lib/types";

function Fact({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div className="grid min-w-0 gap-0.5">
      <dt className="text-xs text-muted-foreground">{label}</dt>
      <dd className="min-w-0 truncate text-sm">{children}</dd>
    </div>
  );
}

/** Dashboard main pane for the selected instance: header, actions, and Services / Connection / Logs / Settings tabs. */
export function InstanceDetail({ model, instance }: { model: AzTrayModel; instance: InstanceSnapshot }) {
  const [confirmDialog, ask] = useConfirm();
  const [tab, setTab] = React.useState("services");
  const { config } = instance;

  // Land on Services when switching instance.
  React.useEffect(() => { setTab("services"); }, [config.id]);

  return (
    <div className="mx-auto grid w-full max-w-5xl gap-5 p-6">
      <header className="flex flex-wrap items-start justify-between gap-4">
        <div className="grid min-w-0 gap-2">
          <div className="flex flex-wrap items-center gap-3">
            <h1 className="truncate text-xl font-semibold tracking-tight">{config.name}</h1>
            <StatusBadge tone={instanceTone(instance.state)} label={instanceStateLabel(instance.state)} />
          </div>
          <PortStrip instance={instance} className="w-fit min-w-72" />
        </div>
        <div className="flex flex-wrap items-center gap-2"><InstanceActions model={model} instance={instance} ask={ask} size="default" /></div>
      </header>

      <Tabs value={tab} onValueChange={setTab} className="gap-4">
        <TabsList variant="line" className="justify-start border-b border-border">
          <TabsTrigger value="services">Services</TabsTrigger>
          <TabsTrigger value="connection">Connection</TabsTrigger>
          <TabsTrigger value="logs">Logs</TabsTrigger>
          <TabsTrigger value="settings">Settings</TabsTrigger>
        </TabsList>

        <TabsContent value="services" className="grid gap-4">
          <Card className="gap-0 py-0">
            <ul className="divide-y divide-border">
              {SERVICE_NAMES.map((name) => (
                <ServiceRow key={name} model={model} service={instance.services[name]} variant="full" confirmFreePort={(service) => ask(freePortRequest(model, service))} />
              ))}
            </ul>
          </Card>
          <Card className="grid gap-4 px-4 sm:grid-cols-3">
            <dl className="contents">
              <Fact label="Host"><span className="inline-flex items-center gap-1.5"><Network className="size-3.5 text-muted-foreground" /><code>{config.host}</code></span></Fact>
              <div className="grid min-w-0 gap-0.5 sm:col-span-2">
                <dt className="text-xs text-muted-foreground">Data folder</dt>
                <dd className="flex min-w-0 items-center gap-1 text-sm">
                  <Database className="size-3.5 shrink-0 text-muted-foreground" aria-hidden="true" />
                  <code className="truncate" title={config.dataDirectory}>{config.dataDirectory}</code>
                  <CopyButton text={config.dataDirectory} label="data folder path" />
                </dd>
              </div>
              <Fact label="Loose mode">{config.loose ? "On" : "Off"}</Fact>
              <Fact label="API version check">{config.skipApiVersionCheck ? "Skipped" : "Enforced"}</Fact>
              <Fact label="Instance id"><code>{config.id}</code></Fact>
            </dl>
          </Card>
        </TabsContent>

        <TabsContent value="connection"><ConnectionPanel instance={instance} /></TabsContent>
        <TabsContent value="logs"><LogViewer model={model} fixedInstanceId={config.id} heightClass="h-[26rem]" /></TabsContent>
        <TabsContent value="settings"><InstanceSettingsTab model={model} instance={instance} /></TabsContent>
      </Tabs>
      {confirmDialog}
    </div>
  );
}
