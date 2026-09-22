import type { Metadata } from "next";
import Script from "next/script";
import "@fontsource-variable/geist";
import "@fontsource-variable/geist-mono";
import "./globals.css";

const themeInitScript = `(() => {
  try {
    const cookie = document.cookie.match(/(?:^|; )aztray-theme=(light|dark|system)(?:;|$)/)?.[1];
    const stored = window.localStorage.getItem("aztray-theme") || cookie;
    const mode = stored === "light" || stored === "dark" || stored === "system" ? stored : "system";
    const prefersDark = window.matchMedia("(prefers-color-scheme: dark)").matches;
    const theme = mode === "system" ? (prefersDark ? "dark" : "light") : mode;
    const root = document.documentElement;
    root.dataset.themeMode = mode;
    root.dataset.theme = theme;
    root.style.colorScheme = theme;
  } catch {
    const cookie = document.cookie.match(/(?:^|; )aztray-theme=(light|dark|system)(?:;|$)/)?.[1];
    const mode = cookie === "light" || cookie === "dark" || cookie === "system" ? cookie : "system";
    const theme = mode === "system" ? (window.matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "light") : mode;
    document.documentElement.dataset.themeMode = mode;
    document.documentElement.dataset.theme = theme;
    document.documentElement.style.colorScheme = theme;
  }
})();`;

export const metadata: Metadata = {
  title: "AzTray",
  description: "A fast tray controller for Azurite",
};

export default function RootLayout({ children }: Readonly<{ children: React.ReactNode }>) {
  return (
    <html lang="en" suppressHydrationWarning>
      <body>
        {children}
        <Script id="aztray-theme-init" strategy="beforeInteractive" dangerouslySetInnerHTML={{ __html: themeInitScript }} />
      </body>
    </html>
  );
}
