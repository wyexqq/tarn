import { For, Match, Show, Switch, createMemo, type JSX } from "solid-js";

import { PANEL_TABS, openPanel, panels, rememberPanelWidth, resizePanel, type PanelTab } from "../panels";
import { ArtifactsTabIcon, BrowserTabIcon, ChangesTabIcon, FilesTabIcon, PlanTabIcon, TasksTabIcon, TerminalTabIcon } from "./icons";
import { ArtifactsPane } from "./panels/ArtifactsPane";
import { BrowserPane } from "./panels/BrowserPane";
import { ChangesPane, FilesPane } from "./panels/FilesPane";
import { PlanPane } from "./panels/PlanPane";
import { TasksPane } from "./panels/TasksPane";
import { TerminalPane } from "./panels/TerminalPane";

const TAB_ICONS: Record<PanelTab, () => JSX.Element> = {
  plan: PlanTabIcon,
  terminal: TerminalTabIcon,
  changes: ChangesTabIcon,
  browser: BrowserTabIcon,
  files: FilesTabIcon,
  tasks: TasksTabIcon,
  artifacts: ArtifactsTabIcon,
};

export function SidePanel() {
  const counts = createMemo<Partial<Record<PanelTab, number>>>(() => ({
    changes: panels.changedFiles().length,
    tasks: panels.tasks().filter((task) => task.finishedAt === null).length,
    plan: panels.plan().filter((step) => step.status !== "done").length,
    artifacts: panels.artifacts().length,
  }));

  const tabTitle = (id: PanelTab, title: string) => {
    const count = counts()[id] ?? 0;
    return count > 0 ? `${title}: ${count}` : title;
  };

  const startResize = (event: PointerEvent) => {
    event.preventDefault();
    const handle = event.currentTarget as HTMLElement;
    const startX = event.clientX;
    const startWidth = panels.width();
    const follow = (moveEvent: PointerEvent) => resizePanel(startWidth + startX - moveEvent.clientX);
    const finish = () => {
      handle.removeEventListener("pointermove", follow);
      handle.removeEventListener("pointerup", finish);
      handle.classList.remove("is-dragging");
      rememberPanelWidth();
    };
    handle.setPointerCapture(event.pointerId);
    handle.classList.add("is-dragging");
    handle.addEventListener("pointermove", follow);
    handle.addEventListener("pointerup", finish);
  };

  return (
    <aside
      class="side-panel"
      classList={{ "is-open": panels.isOpen() }}
      style={{ "--panel-width": `${panels.width()}px` }}
      aria-label="Боковая панель"
      aria-hidden={!panels.isOpen()}
    >
      <div class="side-resize" role="separator" aria-orientation="vertical" aria-label="Ширина панели" onPointerDown={startResize} />
      <div class="side-inner">
        <div class="side-tabs" role="tablist">
          <For each={PANEL_TABS}>
            {(item) => (
              <button
                class="side-tab"
                classList={{ "is-active": panels.tab() === item.id }}
                role="tab"
                title={tabTitle(item.id, item.title)}
                aria-label={tabTitle(item.id, item.title)}
                aria-selected={panels.tab() === item.id}
                tabIndex={panels.isOpen() ? 0 : -1}
                onClick={() => openPanel(item.id)}
              >
                {TAB_ICONS[item.id]()}
                <Show when={(counts()[item.id] ?? 0) > 0}>
                  <span class="side-tab-dot" />
                </Show>
              </button>
            )}
          </For>
        </div>
        <Switch>
          <Match when={panels.tab() === "plan"}><PlanPane /></Match>
          <Match when={panels.tab() === "terminal"}><TerminalPane /></Match>
          <Match when={panels.tab() === "changes"}><ChangesPane /></Match>
          <Match when={panels.tab() === "browser"}><BrowserPane /></Match>
          <Match when={panels.tab() === "files"}><FilesPane /></Match>
          <Match when={panels.tab() === "tasks"}><TasksPane /></Match>
          <Match when={panels.tab() === "artifacts"}><ArtifactsPane /></Match>
        </Switch>
      </div>
    </aside>
  );
}
