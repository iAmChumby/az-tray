"use client";

import * as React from "react";
import { Save, Trash2 } from "lucide-react";
import { Button } from "@/src/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/src/components/ui/card";
import { InstanceFields, portsToNumbers, valuesFromConfig, type InstanceFormValues } from "@/src/components/instances/InstanceFields";
import { useConfirm } from "@/src/components/common/ConfirmDialog";
import type { AzTrayModel } from "@/src/hooks/useAzTray";
import type { InstanceSnapshot } from "@/src/lib/types";
import { nameProblem, portProblems } from "@/src/lib/validation";

export function InstanceSettingsTab({ model, instance }: { model: AzTrayModel; instance: InstanceSnapshot }) {
  const { config } = instance;
  const [values, setValues] = React.useState<InstanceFormValues>(() => valuesFromConfig(config));
  const [confirmDialog, ask] = useConfirm();
  const stopped = instance.state === "stopped";
  const isLast = model.instances.length === 1;

  // Re-sync when the saved config changes (after save, or from another window).
  const serialized = JSON.stringify(config);
  React.useEffect(() => { setValues(valuesFromConfig(config)); }, [serialized]); // eslint-disable-line react-hooks/exhaustive-deps

  const nameError = nameProblem(values.name, model.instances, config.id);
  const portErrors = portProblems(values.ports, model.instances, model.config?.mcp, config.id);
  const dirty = JSON.stringify(values) !== JSON.stringify(valuesFromConfig(config));
  const canSave = dirty && !nameError && Object.keys(portErrors).length === 0 && !!values.dataDirectory.trim() && !model.isPending(config.id);

  const save = async (event: React.FormEvent) => {
    event.preventDefault();
    if (!canSave) return;
    const nameChanged = values.name.trim() !== config.name;
    const structural = JSON.stringify({ ...values, name: "" }) !== JSON.stringify({ ...valuesFromConfig(config), name: "" });
    await model.updateInstance({
      instanceId: config.id,
      ...(nameChanged ? { name: values.name.trim() } : {}),
      ...(structural ? {
        host: values.host.trim(),
        ports: portsToNumbers(values.ports),
        dataDirectory: values.dataDirectory.trim(),
        loose: values.loose,
        skipApiVersionCheck: values.skipApiVersionCheck,
      } : {}),
    });
  };

  const remove = () => ask({
    title: `Delete ${config.name}?`,
    danger: true,
    confirmLabel: "Delete instance",
    body: (
      <div className="grid gap-2">
        <p>AzTray forgets this instance and frees its ports ({config.ports.blob}, {config.ports.queue}, {config.ports.table}).</p>
        <p className="rounded-md bg-muted px-2.5 py-2 text-foreground">
          Your data stays on disk. The folder <code className="break-all">{config.dataDirectory}</code> is not read, changed, or deleted.
        </p>
      </div>
    ),
    action: () => model.deleteInstance(config.id),
  });

  return (
    <div className="grid gap-4">
      <Card>
        <CardHeader>
          <CardTitle>Instance settings</CardTitle>
          <CardDescription>
            {stopped ? "Changes apply the next time this instance starts." : "Stop this instance to change its ports, folder, or flags. The name can change any time."}
          </CardDescription>
        </CardHeader>
        <CardContent>
          <form onSubmit={save} className="grid gap-4">
            <InstanceFields idPrefix={`edit-${config.id}`} values={values} onChange={setValues} nameError={nameError} portErrors={portErrors} idHint={config.id} structureLocked={!stopped} />
            <div className="flex justify-end gap-2">
              <Button type="button" variant="ghost" disabled={!dirty} onClick={() => setValues(valuesFromConfig(config))}>Discard changes</Button>
              <Button type="submit" disabled={!canSave}><Save />Save changes</Button>
            </div>
          </form>
        </CardContent>
      </Card>

      <Card className="ring-status-broken/25">
        <CardHeader>
          <CardTitle>Delete instance</CardTitle>
          <CardDescription>
            Removes {config.name} from AzTray. The data folder on disk is kept as it is.
          </CardDescription>
        </CardHeader>
        <CardContent className="flex flex-wrap items-center justify-between gap-3">
          <p className="max-w-md text-sm text-muted-foreground">
            {isLast ? "AzTray keeps at least one instance, so the last one cannot be deleted." : stopped ? "You can recreate an instance pointing at the same folder later." : "Stop this instance before deleting it."}
          </p>
          <Button type="button" variant="destructive" disabled={!stopped || isLast} onClick={remove}><Trash2 />Delete instance</Button>
        </CardContent>
      </Card>
      {confirmDialog}
    </div>
  );
}
