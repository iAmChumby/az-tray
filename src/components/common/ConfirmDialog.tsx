"use client";

import * as React from "react";
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from "@/src/components/ui/alert-dialog";
import { cn } from "@/src/lib/utils";

export type ConfirmRequest = {
  title: string;
  body: React.ReactNode;
  confirmLabel: string;
  danger?: boolean;
  action: () => void | Promise<unknown>;
};

/** `const [confirmDialog, ask] = useConfirm()`: render `confirmDialog` once, call `ask({...})` anywhere. */
export function useConfirm() {
  const [request, setRequest] = React.useState<ConfirmRequest | null>(null);
  const ask = React.useCallback((next: ConfirmRequest) => setRequest(next), []);
  const dialog = (
    <AlertDialog open={request !== null} onOpenChange={(open) => { if (!open) setRequest(null); }}>
      <AlertDialogContent>
        <AlertDialogHeader>
          <AlertDialogTitle>{request?.title}</AlertDialogTitle>
          <AlertDialogDescription asChild>
            <div>{request?.body}</div>
          </AlertDialogDescription>
        </AlertDialogHeader>
        <AlertDialogFooter>
          <AlertDialogCancel>Cancel</AlertDialogCancel>
          <AlertDialogAction
            className={cn(request?.danger && "bg-destructive text-white hover:bg-destructive/85")}
            onClick={() => { const action = request?.action; setRequest(null); if (action) void action(); }}
          >
            {request?.confirmLabel}
          </AlertDialogAction>
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
  );
  return [dialog, ask] as const;
}
