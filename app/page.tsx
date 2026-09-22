"use client";

import * as React from "react";
import { AlertTriangle, Activity, RotateCw } from "lucide-react";
import { DashboardView, PopoverView } from "@/src/components/AzTrayUI";
import { Button } from "@/src/components/ui/button";
import { useAzTray } from "@/src/hooks/useAzTray";

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
      <main className="az-empty-dashboard az-page-error" role="alert">
        <HeaderMarkFallback />
        <AlertTriangle className="az-page-error-icon" aria-hidden="true" />
        <span className="az-eyebrow">AZTRAY UI ERROR</span>
        <h1>The dashboard hit an error</h1>
        <p>{error.message || "The status response could not be rendered."}</p>
        <details>
          <summary>Show diagnostic</summary>
          <pre>{error.stack ?? String(error)}</pre>
        </details>
        <Button type="button" onClick={() => window.location.reload()}><RotateCw />Reload</Button>
      </main>
    );
  }
}

function HeaderMarkFallback() {
  return <span className="az-app-mark" aria-hidden="true"><Activity className="az-app-mark-icon" /></span>;
}

function PageContent() {
  const mode = useWindowMode();
  const model = useAzTray();
  return mode === "popover" ? <PopoverView model={model} onOpenDashboard={() => void showMainWindow()} onHide={() => void hideCurrentWindow()} /> : <DashboardView model={model} onHide={() => void hideCurrentWindow()} />;
}

export default function Page() {
  return <ClientErrorBoundary><PageContent /></ClientErrorBoundary>;
}
