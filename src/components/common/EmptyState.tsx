import * as React from "react";
import { cn } from "@/src/lib/utils";

export function EmptyState({ icon, title, children, action, className }: { icon: React.ReactNode; title: string; children?: React.ReactNode; action?: React.ReactNode; className?: string }) {
  return (
    <div className={cn("flex flex-col items-center justify-center gap-2 px-6 py-10 text-center", className)}>
      <span aria-hidden="true" className="mb-1 inline-flex size-10 items-center justify-center rounded-xl bg-foreground/6 text-muted-foreground [&_svg]:size-5">{icon}</span>
      <h3 className="text-sm font-medium">{title}</h3>
      {children && <p className="max-w-sm text-sm text-muted-foreground">{children}</p>}
      {action && <div className="mt-2">{action}</div>}
    </div>
  );
}
