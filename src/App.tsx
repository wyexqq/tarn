import { Show, createEffect, onCleanup, onMount } from "solid-js";

import { Chat } from "./components/Chat";
import { DropZone } from "./components/DropZone";
import { HistoryPanel } from "./components/HistoryPanel";
import { ImageViewer } from "./components/ImageView";
import { Palette } from "./components/Palette";
import { Rail } from "./components/Rail";
import { Settings } from "./components/Settings";
import { SidePanel } from "./components/SidePanel";
import { Start } from "./components/Start";
import { Toasts } from "./components/Toasts";
import { setWindowTitle } from "./native";
import { togglePanel } from "./panels";
import { actions, currentChat, goToStart, isNarrowWindow, state, stopStreaming, isStreaming, toggleHistory } from "./store";
import { isZoomBadgeVisible, zoomPercent } from "./zoom";

export function App() {
  const handleKeyDown = (event: KeyboardEvent) => {
    if (event.defaultPrevented) {
      return;
    }
    const key = event.key.toLowerCase();
    if (event.ctrlKey && key === "k") {
      event.preventDefault();
      actions.setPaletteOpen(!state.paletteOpen());
    } else if (event.ctrlKey && key === "n") {
      event.preventDefault();
      goToStart();
    } else if (event.ctrlKey && key === "j") {
      event.preventDefault();
      togglePanel();
    } else if (event.ctrlKey && key === "b") {
      event.preventDefault();
      toggleHistory();
    } else if (event.ctrlKey && (key === "," || event.code === "Comma")) {
      event.preventDefault();
      actions.setSettingsTab(state.settingsTab() ? null : "keys");
    } else if (key === "escape") {
      closeTopmostLayer();
    }
  };

  const closeTopmostLayer = () => {
    if (state.viewedImage()) {
      actions.setViewedImage(null);
    } else if (state.paletteOpen()) {
      actions.setPaletteOpen(false);
    } else if (state.settingsTab()) {
      actions.setSettingsTab(null);
    } else if (state.historyOpen() && isNarrowWindow()) {
      toggleHistory();
    } else if (isStreaming()) {
      void stopStreaming();
    }
  };

  createEffect(() => setWindowTitle(currentChat()?.title ?? null));

  onMount(() => document.addEventListener("keydown", handleKeyDown));
  onCleanup(() => document.removeEventListener("keydown", handleKeyDown));

  return (
    <div class="app">
      <Rail />
      <main class="workspace">
        <HistoryPanel />
        <div class="workspace-main">
          <Show when={state.view() === "start"} fallback={<Chat />}>
            <Start />
          </Show>
        </div>
        <Show when={state.view() === "chat"}>
          <SidePanel />
        </Show>
      </main>
      <DropZone />
      <Palette />
      <Settings />
      <ImageViewer />
      <Toasts />
      <Show when={isZoomBadgeVisible()}>
        <div class="zoom-badge" role="status">{zoomPercent()}%</div>
      </Show>
    </div>
  );
}
