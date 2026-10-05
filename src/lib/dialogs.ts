function isTauri() {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

export const canPickFolder = isTauri;

/** Native folder picker. Returns null when cancelled or when running in a plain browser. */
export async function pickDirectory(defaultPath?: string): Promise<string | null> {
  if (!isTauri()) return null;
  const { open } = await import("@tauri-apps/plugin-dialog");
  const chosen = await open({ directory: true, multiple: false, defaultPath: defaultPath || undefined, title: "Choose a data folder" });
  return typeof chosen === "string" ? chosen : null;
}

/** Native save dialog for log export. Returns undefined in a browser (backend picks a path). */
export async function pickLogSavePath(defaultName: string): Promise<string | null | undefined> {
  if (!isTauri()) return undefined;
  const { save } = await import("@tauri-apps/plugin-dialog");
  return save({ title: "Export AzTray logs", defaultPath: defaultName, filters: [{ name: "Text log", extensions: ["txt"] }] });
}
