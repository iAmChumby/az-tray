import * as React from "react";
import { cn } from "@/src/lib/utils";
import { TONE_DOT, TONE_SOFT, type Tone } from "@/src/lib/status";

/** Small coloured dot. Pulses for the "starting" tone. */
export function StatusDot({ tone, className }: { tone: Tone; className?: string }) {
  return <span aria-hidden="true" className={cn("inline-block size-2 shrink-0 rounded-full", TONE_DOT[tone], className)} />;
}

/** Soft-tinted pill: dot + label. The label is always text, so colour is never the only signal. */
export function StatusBadge({ tone, label, className }: { tone: Tone; label: string; className?: string }) {
  return (
    <span className={cn("inline-flex h-6 shrink-0 items-center gap-1.5 rounded-full px-2.5 text-xs font-medium whitespace-nowrap", TONE_SOFT[tone], className)}>
      <StatusDot tone={tone} className="size-1.5" />
      {label}
    </span>
  );
}
