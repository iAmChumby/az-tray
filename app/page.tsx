"use client";

import * as React from "react";
import { AlertTriangle, RotateCw } from "lucide-react";
import { BrandMark } from "@/src/components/common/AppChrome";
import { Button } from "@/src/components/ui/button";
import { Toaster } from "@/src/components/ui/sonner";
import { TooltipProvider } from "@/src/components/ui/tooltip";
import { DashboardView } from "@/src/components/views/DashboardView";
import { PopoverView } from "@/src/components/views/PopoverView";
import { useAzTray } from "@/src/hooks/useAzTray";
import { useTheme } from "@/src/hooks/useTheme";

type WindowMode = "popover" | "main";

async function hideCurrentWindow() {
  try {
    const { getCurrentWindow } = await import("@tauri-apps/api/window");
    await getCurrentWindow().hide();
  } catch {
    // Browser preview: hiding is represented by returning to the same page.
  }
}

async function showMainWindow() {
  try {
    const { Window, getCurrentWindow } = await import("@tauri-apps/api/window");
    const main = await Window.getByLabel("main");
    await main?.show();
    await main?.setFocus();
    await getCurrentWindow().hide();
  } catch {
    const url = new URL(window.location.href);
    url.searchParams.set("window", "main");
    window.history.replaceState({}, "", url);
    window.dispatchEvent(new PopStateEvent("popstate"));
  }
}

function useWindowMode(): WindowMode {
  const [mode, setMode] = React.useState<WindowMode>("main");
  React.useEffect(() => {
    let alive = true;
    const queryMode = new URLSearchParams(window.location.search).get("window");
    if (queryMode === "popover" || queryMode === "main") setMode(queryMode);
    void import("@tauri-apps/api/window").then(({ getCurrentWindow }) => getCurrentWindow().label).then((label) => {
      if (alive && (label === "popover" || label === "main")) setMode(label);
    }).catch(() => undefined);
    const onPopState = () => {
      const next = new URLSearchParams(window.location.search).get("window");
      if (next === "popover" || next === "main") setMode(next);
    };
    window.addEventListener("popstate", onPopState);
    return () => {
      alive = false;
      window.removeEventListener("popstate", onPopState);
    };
  }, []);
  return mode;
}

type ErrorBoundaryState = { error: Error | null };

class ClientErrorBoundary extends React.Component<React.PropsWithChildren, ErrorBoundaryState> {
  state: ErrorBoundaryState = { error: null };

  static getDerivedStateFromError(error: Error): ErrorBoundaryState {
    return { error };
  }

  componentDidCatch(error: Error, info: React.ErrorInfo) {
    console.error("AzTray UI render failed", error, info.componentStack);
  }

  render() {
    if (!this.state.error) return this.props.children;
    const error = this.state.error;
    return (
      <main className="flex h-dvh flex-col items-center justify-center gap-3 p-8 text-center" role="alert">
        <BrandMark className="size-9" />
        <AlertTriangle className="size-6 text-status-broken" aria-hidden="true" />
        <h1 className="text-lg font-semibold">The window hit an error</h1>
        <p className="max-w-md text-sm text-muted-foreground">{error.message || "The status response could not be shown."}</p>
        <details className="max-w-xl text-left text-xs">
          <summary className="cursor-pointer text-muted-foreground">Show details</summary>
          <pre className="az-scroll mt-2 max-h-48 overflow-auto rounded-lg bg-muted p-3">{error.stack ?? String(error)}</pre>
        </details>
        <Button type="button" onClick={() => window.location.reload()}><RotateCw />Reload</Button>
      </main>
    );
  }
}

function PageContent() {
  const mode = useWindowMode();
  const model = useAzTray();
  const { theme } = useTheme();
  return (
    <TooltipProvider delayDuration={300}>
      {mode === "popover"
        ? <PopoverView model={model} onOpenDashboard={() => void showMainWindow()} onHide={() => void hideCurrentWindow()} />
        : <DashboardView model={model} onHide={() => void hideCurrentWindow()} />}
      <Toaster theme={theme} position={mode === "popover" ? "top-center" : "bottom-right"} closeButton />
    </TooltipProvider>
  );
}

export default function Page() {
  return <ClientErrorBoundary><PageContent /></ClientErrorBoundary>;
}
