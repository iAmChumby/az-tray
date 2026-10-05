"use client";

import * as React from "react";
import { Info } from "lucide-react";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/src/components/ui/card";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/src/components/ui/tabs";
import { CodeBlock } from "@/src/components/common/CodeBlock";
import { CopyButton } from "@/src/components/common/CopyButton";
import { ServiceGlyph } from "@/src/components/common/ServiceGlyph";
import { connectionSnippet, SNIPPET_LABELS, type SnippetFormat } from "@/src/lib/format";
import { SERVICE_LABELS, SERVICE_NAMES, type InstanceSnapshot } from "@/src/lib/types";

const FORMATS: SnippetFormat[] = ["env", "powershell", "appsettings", "localSettings"];

/** Everything needed to point an app, SDK, or test run at this instance. */
export function ConnectionPanel({ instance }: { instance: InstanceSnapshot }) {
  const info = instance.connection;
  return (
    <div className="grid gap-4">
      {instance.state !== "running" && (
        <p className="flex items-start gap-2 rounded-lg bg-muted px-3 py-2 text-sm text-muted-foreground" role="status">
          <Info className="mt-0.5 size-4 shrink-0" aria-hidden="true" />
          {instance.config.name} is not fully running. These values are valid and start working once its services are up.
        </p>
      )}

      <Card>
        <CardHeader>
          <CardTitle>Connection string</CardTitle>
          <CardDescription>One string covering Blob, Queue, and Table for this instance. Account <code>{info.accountName}</code>.</CardDescription>
        </CardHeader>
        <CardContent>
          <CodeBlock code={info.connectionString} label="connection string" caption="Combined connection string" wrap />
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle>Service endpoints</CardTitle>
          <CardDescription>Copy a single endpoint, or a connection string that targets just one service.</CardDescription>
        </CardHeader>
        <CardContent className="px-0">
          <ul className="divide-y divide-border">
            {SERVICE_NAMES.map((name) => (
              <li key={name} className="flex items-center gap-3 px-4 py-2.5">
                <ServiceGlyph name={name} />
                <div className="min-w-0 flex-1">
                  <strong className="block text-sm font-medium">{SERVICE_LABELS[name]}</strong>
                  <code className="block truncate text-[13px] text-muted-foreground">{info.endpoints[name]}</code>
                </div>
                <CopyButton variant="labeled" buttonVariant="outline" text={info.endpoints[name]} label={`${SERVICE_LABELS[name]} endpoint`}>Endpoint</CopyButton>
                <CopyButton variant="labeled" buttonVariant="outline" text={info.connectionStrings[name]} label={`${SERVICE_LABELS[name]} connection string`}>Connection string</CopyButton>
              </li>
            ))}
          </ul>
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle>Use it in your app</CardTitle>
          <CardDescription>Ready-made snippets with this instance&apos;s connection string.</CardDescription>
        </CardHeader>
        <CardContent>
          <Tabs defaultValue="env">
            <TabsList variant="line" className="justify-start">
              {FORMATS.map((format) => <TabsTrigger key={format} value={format}>{SNIPPET_LABELS[format]}</TabsTrigger>)}
            </TabsList>
            {FORMATS.map((format) => (
              <TabsContent key={format} value={format} className="mt-2">
                <CodeBlock code={connectionSnippet(format, info)} label={`${SNIPPET_LABELS[format]} snippet`} caption={format === "env" ? "Environment variable" : SNIPPET_LABELS[format]} wrap={format === "env" || format === "powershell"} />
              </TabsContent>
            ))}
          </Tabs>
        </CardContent>
      </Card>
    </div>
  );
}
