import { render } from "solid-js/web";

import { App } from "./App";
import { api } from "./api";
import { showEditingContextMenu, showSelectionContextMenu, targetsTextEditing } from "./native";
import { initializePanels } from "./panels";
import { initializeStore, showToast } from "./store";
import "./styles/tokens.css";
import "./styles/base.css";
import "./styles/layout.css";
import "./styles/chat.css";
import "./styles/overlays.css";
import "./styles/panels.css";
import "./styles/widgets.css";

function showWindowAfterFirstPaint(): void {
  requestAnimationFrame(() => {
    requestAnimationFrame(() => {
      void api.showMainWindow();
    });
  });
}

function blockBrowserBehaviour(): void {
  document.addEventListener("contextmenu", (event) => {
    event.preventDefault();
    const isEditable = (event.target as HTMLElement).closest("input, textarea") !== null;
    if (isEditable) {
      void showEditingContextMenu();
    } else if (targetsTextEditing(event)) {
      void showSelectionContextMenu();
    }
  });
  document.addEventListener("keydown", (event) => {
    const isReload = event.key === "F5" || (event.ctrlKey && event.key.toLowerCase() === "r");
    const isBrowserShortcut = event.ctrlKey && ["p", "u", "g", "s", "h", "d"].includes(event.key.toLowerCase());
    if (isReload || isBrowserShortcut) {
      event.preventDefault();
    }
  });
  document.addEventListener("dragover", (event) => event.preventDefault());
  document.addEventListener("drop", (event) => event.preventDefault());
}

async function start(): Promise<void> {
  blockBrowserBehaviour();
  try {
    await initializeStore();
    await initializePanels();
  } catch (error) {
    showToast(`Не удалось запустить приложение: ${error}`, "error");
  }
  const root = document.getElementById("root");
  if (root) {
    render(() => <App />, root);
  }
  showWindowAfterFirstPaint();
}

void start();
