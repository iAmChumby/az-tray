"use client";

import * as React from "react";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/src/components/ui/tabs";
import { AppLogPanel } from "@/src/components/logs/AppLogPanel";
import { LogViewer } from "@/src/components/logs/LogViewer";
import type { AzTrayModel } from "@/src/hooks/useAzTray";

export function LogsView({ model }: { model: AzTrayModel }) {
  return (
    <div className="mx-auto grid w-full max-w-5xl gap-5 p-6">
      <header className="grid gap-1">
        <h1 className="text-xl font-semibold tracking-tight">Logs</h1>
        <p className="text-sm text-muted-foreground">Azurite output for every instance, plus AzTray&apos;s own diagnostic log.</p>
      </header>
      <Tabs defaultValue="azurite" className="gap-4">
        <TabsList variant="line" className="justify-start border-b border-border">
          <TabsTrigger value="azurite">Azurite output</TabsTrigger>
          <TabsTrigger value="app">AzTray app log</TabsTrigger>
        </TabsList>
        <TabsContent value="azurite"><LogViewer model={model} heightClass="h-[30rem]" /></TabsContent>
        <TabsContent value="app"><AppLogPanel logPath={model.app?.logPath} heightClass="h-[30rem]" /></TabsContent>
      </Tabs>
    </div>
  );
}
