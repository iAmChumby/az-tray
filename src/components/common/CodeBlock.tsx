import * as React from "react";
import { CopyButton } from "@/src/components/common/CopyButton";
import { cn } from "@/src/lib/utils";

/** Monospace snippet with a copy action. Long lines scroll instead of wrapping so values stay copy-exact. */
export function CodeBlock({ code, label, caption, className, wrap = false }: { code: string; label: string; caption?: string; className?: string; wrap?: boolean }) {
  return (
    <figure className={cn("overflow-hidden rounded-lg border border-border bg-muted", className)}>
      <div className="flex items-center justify-between gap-2 border-b border-border px-3 py-1">
        <figcaption className="truncate text-xs text-muted-foreground">{caption ?? label}</figcaption>
        <CopyButton text={code} label={label} variant="labeled" size="xs" />
      </div>
      <pre className={cn("az-scroll max-h-64 overflow-auto px-3 py-2.5 font-mono text-[12.5px] leading-relaxed", wrap ? "whitespace-pre-wrap break-all" : "whitespace-pre")} tabIndex={0}>
        {code}
      </pre>
    </figure>
  );
}
