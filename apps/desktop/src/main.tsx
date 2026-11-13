import React, { type ComponentType } from "react";
import { createRoot } from "react-dom/client";
import { isTauri } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { markNativeWindow } from "./app/nativeWindow";
import { applyAppearanceToDocument, browserStorage, loadAppearance } from "./features/settings/appearance";
import "./app/tokens.css";

// Apply the saved appearance and window mode before the first paint so the page never
// flashes the wrong theme or an opaque sidebar.
applyAppearanceToDocument(document.documentElement, loadAppearance(browserStorage()).preference);
markNativeWindow(document.documentElement, isTauri());

const rootElement = document.getElementById("root");

if (!rootElement) {
  throw new Error("Root element is missing");
}

const root = createRoot(rootElement);

// The menu-bar popover is its own window (label "popover", see src-tauri/src/menubar.rs).
// Otherwise development builds load V2App, the release interface with a development badge,
// and release builds ReleaseApp: Vite replaces import.meta.env.DEV with false there, so
// V2App and src/v2/sample/ are dropped from the bundle (checked after build by
// scripts/check-release-bundle.mjs).
const popover = isTauri() && getCurrentWindow().label === "popover";
if (popover) {
  // Before the first paint, so the page never lays out at the main window's minimum width.
  document.documentElement.dataset.window = "popover";
}
const load: Promise<ComponentType> = popover
  ? import("./v2/PopoverApp").then((module) => module.PopoverApp)
  : import.meta.env.DEV
    ? import("./v2/V2App").then((module) => module.V2App)
    : import("./v2/ReleaseApp").then((module) => module.ReleaseApp);

load.then(
  (Root) =>
    root.render(
      <React.StrictMode>
        <Root />
      </React.StrictMode>,
    ),
  (error: unknown) => {
    rootElement.textContent = "Kleari could not start. Quit and open it again.";
    throw error;
  },
);
