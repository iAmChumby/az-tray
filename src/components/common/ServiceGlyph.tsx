import * as React from "react";
import { Box, ListChecks, Table2 } from "lucide-react";
import { cn } from "@/src/lib/utils";
import type { ServiceName } from "@/src/lib/types";

const META: Record<ServiceName, { icon: React.ComponentType<{ className?: string }>; tint: string; text: string; description: string }> = {
  blob: { icon: Box, tint: "bg-svc-blob/14", text: "text-svc-blob", description: "Object storage" },
  queue: { icon: ListChecks, tint: "bg-svc-queue/14", text: "text-svc-queue", description: "Message queues" },
  table: { icon: Table2, tint: "bg-svc-table/14", text: "text-svc-table", description: "NoSQL tables" },
};

export const SERVICE_DESCRIPTIONS: Record<ServiceName, string> = { blob: META.blob.description, queue: META.queue.description, table: META.table.description };
export const SERVICE_TEXT: Record<ServiceName, string> = { blob: META.blob.text, queue: META.queue.text, table: META.table.text };

export function ServiceGlyph({ name, className }: { name: ServiceName; className?: string }) {
  const { icon: Icon, tint, text } = META[name];
  return (
    <span aria-hidden="true" className={cn("inline-flex size-8 shrink-0 items-center justify-center rounded-lg", tint, text, className)}>
      <Icon className="size-4" />
    </span>
  );
}
