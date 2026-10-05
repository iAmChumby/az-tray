"use client";

import * as React from "react";
import { LoaderCircle, Play, RotateCw, Square } from "lucide-react";
import { Button } from "@/src/components/ui/button";
import type { AzTrayModel } from "@/src/hooks/useAzTray";
import type { ConfirmRequest } from "@/src/components/common/ConfirmDialog";
import type { InstanceSnapshot } from "@/src/lib/types";

/** Start / Stop / Restart for one instance, shown according to its aggregate state. */
export function InstanceActions({ model, instance, ask, size = "sm" }: { model: AzTrayModel; instance: InstanceSnapshot; ask: (request: ConfirmRequest) => void; size?: "sm" | "default" }) {
  const id = instance.config.id;
  const name = instance.config.name;
  const busy = model.isPending(id) || model.isPending("all") || instance.state === "starting";
  const ready = model.engine.state === "ready";
  const showStart = instance.state !== "running";
  const showStop = instance.state !== "stopped";
  if (busy) {
    return <Button type="button" size={size} variant="secondary" disabled><LoaderCircle className="animate-spin" />Working</Button>;
  }
  return (
    <>
      {showStart && <Button type="button" size={size} onClick={() => void model.startInstance(id)} disabled={!ready}><Play />{instance.state === "stopped" ? "Start" : "Start the rest"}</Button>}
      {showStop && <Button type="button" size={size} variant="secondary" onClick={() => void model.stopInstance(id)}><Square />Stop</Button>}
      {showStop && (
        <Button
          type="button"
          size={size}
          variant="secondary"
          disabled={!ready}
          onClick={() => ask({ title: `Restart ${name}?`, body: "Its three services stop briefly and start again. Connected clients will see dropped connections.", confirmLabel: "Restart", action: () => model.restartInstance(id) })}
        >
          <RotateCw />Restart
        </Button>
      )}
    </>
  );
}
