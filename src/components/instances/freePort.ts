import type { ConfirmRequest } from "@/src/components/common/ConfirmDialog";
import type { AzTrayModel } from "@/src/hooks/useAzTray";
import { SERVICE_LABELS, type ServiceSnapshot } from "@/src/lib/types";

/** Confirmation prompt shared by every surface that offers "Free port". */
export function freePortRequest(model: AzTrayModel, service: ServiceSnapshot): ConfirmRequest {
  const owner = service.portOwner;
  return {
    title: `Free port ${service.port}?`,
    danger: true,
    confirmLabel: "Free port",
    body: owner
      ? `${owner.name ?? "A process"} (PID ${owner.pid}) is listening on ${SERVICE_LABELS[service.name]} port ${service.port}. AzTray checks that it is still the same process, then ends it.`
      : "AzTray checks what is listening on this port before it takes any action.",
    action: () => model.freePort(service.instanceId, service.name),
  };
}
