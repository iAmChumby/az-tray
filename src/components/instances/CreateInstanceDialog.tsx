"use client";

import * as React from "react";
import { LoaderCircle, Plus } from "lucide-react";
import { Button } from "@/src/components/ui/button";
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "@/src/components/ui/dialog";
import { Label } from "@/src/components/ui/label";
import { Switch } from "@/src/components/ui/switch";
import { InstanceFields, portsToNumbers, type InstanceFormValues } from "@/src/components/instances/InstanceFields";
import type { AzTrayModel } from "@/src/hooks/useAzTray";
import { aztrayIpc } from "@/src/lib/ipc";
import { nameProblem, portProblems } from "@/src/lib/validation";

const BLANK: InstanceFormValues = {
  name: "",
  host: "127.0.0.1",
  ports: { blob: "", queue: "", table: "" },
  dataDirectory: "",
  loose: false,
  skipApiVersionCheck: false,
};

/** "New instance" form, prefilled from suggest_instance (next free ports, unique name, data folder). */
export function CreateInstanceDialog({ model, open, onOpenChange }: { model: AzTrayModel; open: boolean; onOpenChange: (open: boolean) => void }) {
  const [values, setValues] = React.useState<InstanceFormValues>(BLANK);
  const [id, setId] = React.useState("");
  const [startNow, setStartNow] = React.useState(true);
  const [loading, setLoading] = React.useState(false);
  const [submitting, setSubmitting] = React.useState(false);
  const dirEdited = React.useRef(false);
  const draftReady = React.useRef(false);

  // Prefill each time the dialog opens.
  React.useEffect(() => {
    if (!open) return;
    let alive = true;
    dirEdited.current = false;
    draftReady.current = false;
    setLoading(true);
    setStartNow(true);
    aztrayIpc.suggestInstance().then((draft) => {
      if (!alive) return;
      setValues({
        name: draft.name,
        host: draft.host,
        ports: { blob: String(draft.ports.blob), queue: String(draft.ports.queue), table: String(draft.ports.table) },
        dataDirectory: draft.dataDirectory,
        loose: false,
        skipApiVersionCheck: false,
      });
      setId(draft.id);
      draftReady.current = true;
    }).catch(() => undefined).finally(() => { if (alive) setLoading(false); });
    return () => { alive = false; };
  }, [open]);

  // Keep the generated id (and untouched data folder) in step with the name.
  React.useEffect(() => {
    if (!open || !draftReady.current || !values.name.trim()) return;
    let alive = true;
    const timer = window.setTimeout(() => {
      aztrayIpc.suggestInstance(values.name.trim()).then((draft) => {
        if (!alive) return;
        setId(draft.id);
        if (!dirEdited.current) setValues((current) => ({ ...current, dataDirectory: draft.dataDirectory }));
      }).catch(() => undefined);
    }, 250);
    return () => { alive = false; window.clearTimeout(timer); };
  }, [open, values.name]);

  const nameError = values.name ? nameProblem(values.name, model.instances) : null;
  const portErrors = portProblems(values.ports, model.instances, model.config?.mcp);
  const hasPortErrors = Object.keys(portErrors).length > 0;
  const canSubmit = !!values.name.trim() && !nameError && !hasPortErrors && !loading && !submitting && !!values.dataDirectory.trim();

  const submit = async (event: React.FormEvent) => {
    event.preventDefault();
    if (!canSubmit) return;
    setSubmitting(true);
    const created = await model.createInstance({
      name: values.name.trim(),
      id: id || undefined,
      host: values.host.trim() || undefined,
      ports: portsToNumbers(values.ports),
      dataDirectory: values.dataDirectory.trim(),
      loose: values.loose,
      skipApiVersionCheck: values.skipApiVersionCheck,
      start: startNow,
    });
    setSubmitting(false);
    if (created) onOpenChange(false);
  };

  return (
    <Dialog open={open} onOpenChange={(next) => { if (!submitting) onOpenChange(next); }}>
      <DialogContent className="max-h-[90vh] gap-0 overflow-y-auto p-0 sm:max-w-lg">
        <form onSubmit={submit} className="grid gap-4 p-5">
          <DialogHeader>
            <DialogTitle>New Azurite instance</DialogTitle>
            <DialogDescription>An instance is one Blob, Queue, and Table set with its own ports and data folder. It runs alongside your other instances.</DialogDescription>
          </DialogHeader>
          {loading ? (
            <div className="flex items-center gap-2 py-10 text-sm text-muted-foreground"><LoaderCircle className="size-4 animate-spin" />Finding free ports</div>
          ) : (
            <InstanceFields idPrefix="create" values={values} onChange={setValues} nameError={nameError} portErrors={portErrors} idHint={id} onDataDirectoryEdited={() => { dirEdited.current = true; }} />
          )}
          <div className="flex items-center justify-between gap-4 rounded-lg bg-muted px-3 py-2.5">
            <div className="grid gap-0.5">
              <Label htmlFor="create-start">Start after creating</Label>
              <p className="text-xs text-muted-foreground">Launch all three services right away.</p>
            </div>
            <Switch id="create-start" checked={startNow} onCheckedChange={setStartNow} />
          </div>
          <DialogFooter>
            <Button type="button" variant="ghost" onClick={() => onOpenChange(false)} disabled={submitting}>Cancel</Button>
            <Button type="submit" disabled={!canSubmit}>
              {submitting ? <LoaderCircle className="animate-spin" /> : <Plus />}
              {startNow ? "Create and start" : "Create instance"}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}
