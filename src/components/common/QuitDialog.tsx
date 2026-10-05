"use client";

import * as React from "react";
import { ChevronRight, Cloud, Square } from "lucide-react";
import { Button } from "@/src/components/ui/button";
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "@/src/components/ui/dialog";
import type { AzTrayModel } from "@/src/hooks/useAzTray";
import { pluralize } from "@/src/lib/format";

export function QuitDialog({ model, open, onOpenChange, onDone }: { model: AzTrayModel; open: boolean; onOpenChange: (open: boolean) => void; onDone: () => void }) {
  const [busy, setBusy] = React.useState(false);
  const running = model.runningInstanceCount;
  const choose = async (mode: "stop_and_quit" | "leave_running") => {
    setBusy(true);
    const quit = await model.quit(mode);
    setBusy(false);
    if (quit) onDone();
  };
  return (
    <Dialog open={open} onOpenChange={(next) => { if (!busy) onOpenChange(next); }}>
      <DialogContent className="sm:max-w-md">
        <DialogHeader>
          <DialogTitle>Quit AzTray</DialogTitle>
          <DialogDescription>
            {running > 0 ? `${pluralize(running, "instance")} running. Choose what happens to ${running === 1 ? "it" : "them"}.` : "No instances are running."}
          </DialogDescription>
        </DialogHeader>
        <div className="grid gap-2">
          <Button type="button" variant="outline" className="h-auto justify-start gap-3 px-3 py-2.5 text-left" disabled={busy} onClick={() => void choose("stop_and_quit")}>
            <Square className="text-muted-foreground" />
            <span className="grid flex-1 gap-0.5">
              <span className="font-medium">Stop everything and quit</span>
              <span className="text-xs font-normal text-muted-foreground">Stops every service AzTray started, then exits.</span>
            </span>
            <ChevronRight className="text-faint" />
          </Button>
          <Button type="button" variant="outline" className="h-auto justify-start gap-3 px-3 py-2.5 text-left" disabled={busy} onClick={() => void choose("leave_running")}>
            <Cloud className="text-muted-foreground" />
            <span className="grid flex-1 gap-0.5">
              <span className="font-medium">Quit and leave Azurite running</span>
              <span className="text-xs font-normal text-muted-foreground">Services keep serving. The MCP server stops with AzTray.</span>
            </span>
            <ChevronRight className="text-faint" />
          </Button>
        </div>
        <DialogFooter>
          <Button type="button" variant="ghost" disabled={busy} onClick={() => onOpenChange(false)}>Cancel</Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
