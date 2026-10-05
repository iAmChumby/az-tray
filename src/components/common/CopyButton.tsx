"use client";

import * as React from "react";
import { Check, Copy } from "lucide-react";
import { toast } from "sonner";
import { Button } from "@/src/components/ui/button";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/src/components/ui/tooltip";
import { copyText } from "@/src/lib/clipboard";
import { actionErrorMessage } from "@/src/lib/actionError";
import { cn } from "@/src/lib/utils";

type CopyButtonProps = {
  /** Text to copy, or a function evaluated at click time. */
  text: string | (() => string);
  /** Names what is copied: used for the tooltip, aria-label and failure toast. */
  label: string;
  /** "icon" renders a square icon button; "labeled" shows text next to the icon. */
  variant?: "icon" | "labeled";
  buttonVariant?: React.ComponentProps<typeof Button>["variant"];
  size?: React.ComponentProps<typeof Button>["size"];
  className?: string;
  children?: React.ReactNode;
};

export function CopyButton({ text, label, variant = "icon", buttonVariant = "ghost", size, className, children }: CopyButtonProps) {
  const [copied, setCopied] = React.useState(false);
  const timer = React.useRef<number | undefined>(undefined);
  React.useEffect(() => () => window.clearTimeout(timer.current), []);

  const onClick = async () => {
    try {
      await copyText(typeof text === "function" ? text() : text);
      setCopied(true);
      window.clearTimeout(timer.current);
      timer.current = window.setTimeout(() => setCopied(false), 1600);
    } catch (cause) {
      toast.error(`Unable to copy ${label}`, { description: actionErrorMessage(cause, "Clipboard is unavailable") });
    }
  };

  const icon = copied ? <Check className="text-status-running" /> : <Copy />;
  if (variant === "labeled") {
    return (
      <Button type="button" variant={buttonVariant} size={size ?? "sm"} className={className} onClick={onClick} aria-label={`Copy ${label}`}>
        {icon}
        <span aria-live="polite">{copied ? "Copied" : children ?? "Copy"}</span>
      </Button>
    );
  }
  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <Button type="button" variant={buttonVariant} size={size ?? "icon-sm"} className={cn("text-muted-foreground hover:text-foreground", className)} onClick={onClick} aria-label={`Copy ${label}`}>
          {icon}
        </Button>
      </TooltipTrigger>
      <TooltipContent>{copied ? "Copied" : `Copy ${label}`}</TooltipContent>
    </Tooltip>
  );
}
