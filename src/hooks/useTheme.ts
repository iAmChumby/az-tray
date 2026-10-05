"use client";

import * as React from "react";

const THEME_STORAGE_KEY = "aztray-theme";
const THEME_CHANNEL_NAME = "aztray-theme";
export type ThemeMode = "light" | "dark" | "system";
export type EffectiveTheme = "light" | "dark";

function isThemeMode(value: string | null | undefined): value is ThemeMode {
  return value === "light" || value === "dark" || value === "system";
}

function systemTheme(): EffectiveTheme {
  return typeof window !== "undefined" && window.matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "light";
}

function initialThemeMode(): ThemeMode {
  if (typeof document === "undefined") return "system";
  const mode = document.documentElement.dataset.themeMode;
  return isThemeMode(mode) ? mode : "system";
}

function initialEffectiveTheme(): EffectiveTheme {
  if (typeof document !== "undefined") {
    const theme = document.documentElement.dataset.theme;
    if (theme === "light" || theme === "dark") return theme;
  }
  return systemTheme();
}

/** Light / dark / system theme, synced across the popover and dashboard windows. */
export function useTheme() {
  const [mode, setModeState] = React.useState<ThemeMode>(initialThemeMode);
  const [theme, setThemeState] = React.useState<EffectiveTheme>(initialEffectiveTheme);
  const channelRef = React.useRef<BroadcastChannel | null>(null);

  const applyTheme = React.useCallback((next: ThemeMode) => {
    const effective = next === "system" ? systemTheme() : next;
    document.documentElement.dataset.themeMode = next;
    document.documentElement.dataset.theme = effective;
    document.documentElement.style.colorScheme = effective;
    setThemeState(effective);
  }, []);

  React.useEffect(() => {
    applyTheme(mode);
    const receive = (next: ThemeMode) => { setModeState(next); applyTheme(next); };
    const onStorage = (event: StorageEvent) => { if (event.key === THEME_STORAGE_KEY && isThemeMode(event.newValue)) receive(event.newValue); };
    const media = window.matchMedia("(prefers-color-scheme: dark)");
    const onSystemChange = () => { if (mode === "system") applyTheme("system"); };
    window.addEventListener("storage", onStorage);
    media.addEventListener?.("change", onSystemChange);
    if ("BroadcastChannel" in window) {
      const channel = new BroadcastChannel(THEME_CHANNEL_NAME);
      channelRef.current = channel;
      channel.onmessage = (event: MessageEvent<unknown>) => {
        const next = typeof event.data === "string" ? event.data : (event.data as { mode?: unknown } | null)?.mode;
        if (typeof next === "string" && isThemeMode(next)) receive(next);
      };
    }
    return () => {
      window.removeEventListener("storage", onStorage);
      media.removeEventListener?.("change", onSystemChange);
      channelRef.current?.close();
      channelRef.current = null;
    };
  }, [applyTheme, mode]);

  const setMode = React.useCallback((next: ThemeMode) => {
    setModeState(next);
    applyTheme(next);
    try { window.localStorage.setItem(THEME_STORAGE_KEY, next); } catch { /* previews may disable storage */ }
    try { document.cookie = `${THEME_STORAGE_KEY}=${next}; max-age=31536000; path=/; SameSite=Lax`; } catch { /* previews may disable cookies */ }
    channelRef.current?.postMessage(next);
  }, [applyTheme]);

  const toggle = React.useCallback(() => setMode(theme === "dark" ? "light" : "dark"), [setMode, theme]);
  return { mode, theme, setMode, toggle };
}
