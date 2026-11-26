import { createEffect, createRoot, createSignal, on } from "solid-js";
import { createStore } from "solid-js/store";
import { listen } from "@tauri-apps/api/event";

import { api, type Artifact, type BackgroundTask, type BrowserView, type ChangedFile, type ContextUsage, type PlanStep, type TerminalView } from "./api";
import { formatBytes } from "./format";
import { chosenModels, showToast, state } from "./store";

export type PanelTab = "plan" | "terminal" | "changes" | "browser" | "files" | "tasks" | "artifacts";
export type TerminalMode = "model" | "mine";

export const PANEL_TABS: { id: PanelTab; title: string }[] = [
  { id: "plan", title: "План" },
  { id: "terminal", title: "Терминал" },
  { id: "changes", title: "Изменения" },
  { id: "browser", title: "Браузер" },
  { id: "files", title: "Файлы" },
  { id: "tasks", title: "Задачи" },
  { id: "artifacts", title: "Артефакты" },
];

const ARTIFACT_KIND_NAMES: Record<string, string> = {
  html: "Страница",
  markdown: "Документ",
  text: "Текст",
  csv: "Таблица",
  svg: "Картинка",
  json: "Данные",
  code: "Код",
  file: "Файл",
};

const TOOL_PANELS: Record<string, PanelTab> = {
  run_command: "terminal",
  Bash: "terminal",
  shell: "terminal",
  exec_command: "terminal",
  start_background: "tasks",
  read_background: "tasks",
  stop_background: "tasks",
  browser_open: "browser",
  browser_search: "browser",
  browser_read: "browser",
  browser_click: "browser",
  browser_type: "browser",
  write_file: "changes",
  edit_file: "changes",
  Write: "changes",
  Edit: "changes",
  update_plan: "plan",
  create_artifact: "artifacts",
  send_file: "artifacts",
};

const DEFAULT_PANEL_WIDTH = 460;
const MIN_PANEL_WIDTH = 320;
const MAX_PANEL_SHARE = 0.65;
const MAX_KEPT_OUTPUT_CHARS = 200_000;
const EMPTY_TERMINAL: TerminalView = { text: "", folder: "", isRunning: false };
const EMPTY_BROWSER: BrowserView = { url: "", title: "", isLoading: false, activity: "" };

const [isOpen, setIsOpen] = createSignal(false);
const [tab, setTab] = createSignal<PanelTab>("terminal");
const [width, setWidth] = createSignal(DEFAULT_PANEL_WIDTH);
const [terminalMode, setTerminalMode] = createSignal<TerminalMode>("model");
const [liveOutput, setLiveOutput] = createStore<Record<string, string>>({});
const [tasks, setTasks] = createSignal<BackgroundTask[]>([]);
const [taskOutputs, setTaskOutputs] = createStore<Record<string, string>>({});
const [myTerminal, setMyTerminal] = createSignal<TerminalView>(EMPTY_TERMINAL);
const [browser, setBrowser] = createSignal<BrowserView>(EMPTY_BROWSER);
const [plan, setPlan] = createSignal<PlanStep[]>([]);
const [artifacts, setArtifacts] = createSignal<Artifact[]>([]);
const [selectedArtifactId, setSelectedArtifactId] = createSignal<string | null>(null);
const [changedFiles, setChangedFiles] = createSignal<ChangedFile[]>([]);
const [contextUsage, setContextUsage] = createSignal<ContextUsage | null>(null);
const [isCompacting, setIsCompacting] = createSignal(false);

export const panels = {
  isOpen,
  tab,
  width,
  terminalMode,
  liveOutput,
  tasks,
  taskOutputs,
  myTerminal,
  browser,
  plan,
  artifacts,
  selectedArtifactId,
  changedFiles,
  contextUsage,
  isCompacting,
};

function limitWidth(value: number): number {
  const widest = Math.max(MIN_PANEL_WIDTH, window.innerWidth * MAX_PANEL_SHARE);
  return Math.round(Math.min(widest, Math.max(MIN_PANEL_WIDTH, value)));
}

function appendLimited(previous: string | undefined, text: string): string {
  const joined = (previous ?? "") + text;
  return joined.length > MAX_KEPT_OUTPUT_CHARS ? joined.slice(joined.length - MAX_KEPT_OUTPUT_CHARS) : joined;
}

function isCurrentChat(chatId: string): boolean {
  return state.currentChatId() === chatId;
}

function currentModelRef(): string | null {
  return chosenModels()[0] ?? state.selectedModel();
}

export function openPanel(next: PanelTab): void {
  setTab(next);
  setIsOpen(true);
  void api.setSetting("panelOpen", "true");
  void api.setSetting("panelTab", next);
}

export function togglePanel(): void {
  const open = !isOpen();
  setIsOpen(open);
  void api.setSetting("panelOpen", String(open));
}

export function resizePanel(value: number): void {
  setWidth(limitWidth(value));
}

export function rememberPanelWidth(): void {
  void api.setSetting("panelWidth", String(width()));
}

export function chooseTerminalMode(mode: TerminalMode): void {
  setTerminalMode(mode);
}

export function showArtifact(artifactId: string): void {
  setSelectedArtifactId(artifactId);
  openPanel("artifacts");
}

export function describeArtifact(artifact: Artifact): string {
  return `${ARTIFACT_KIND_NAMES[artifact.kind] ?? "Файл"} · ${formatBytes(artifact.size)}`;
}

export function panelOfTool(toolName: string): PanelTab | null {
  return TOOL_PANELS[toolName] ?? null;
}

export function showToolInPanel(toolName: string): void {
  const target = panelOfTool(toolName);
  if (target === "terminal") {
    setTerminalMode("model");
  }
  if (target) {
    openPanel(target);
  }
}

export function chooseArtifact(artifactId: string | null): void {
  setSelectedArtifactId(artifactId);
}

export async function refreshContextUsage(): Promise<void> {
  const chatId = state.currentChatId();
  const modelRef = currentModelRef();
  if (!chatId || !modelRef) {
    setContextUsage(null);
    return;
  }
  try {
    const usage = await api.contextUsage(chatId, modelRef);
    if (isCurrentChat(chatId)) {
      setContextUsage(usage);
    }
  } catch {
    setContextUsage(null);
  }
}

export async function compactNow(): Promise<void> {
  const chatId = state.currentChatId();
  const modelRef = currentModelRef();
  if (!chatId || !modelRef || isCompacting()) {
    return;
  }
  setIsCompacting(true);
  try {
    const note = await api.compactChat(chatId, modelRef);
    showToast(note ?? "Сжимать пока нечего");
    await refreshContextUsage();
  } catch (error) {
    showToast(String(error), "error");
  } finally {
    setIsCompacting(false);
  }
}

export async function refreshChangedFiles(): Promise<void> {
  const chatId = state.currentChatId();
  if (!chatId) {
    setChangedFiles([]);
    return;
  }
  const files = await api.changedFiles(chatId).catch(() => []);
  if (isCurrentChat(chatId)) {
    setChangedFiles(files);
  }
}

export async function loadTaskOutput(taskId: string): Promise<void> {
  const text = await api.backgroundOutput(taskId).catch(() => "");
  setTaskOutputs(taskId, text);
}

export async function stopTask(taskId: string): Promise<void> {
  await api.stopBackgroundTask(taskId).catch((error) => showToast(String(error), "error"));
}

export async function runInMyTerminal(line: string): Promise<void> {
  const chatId = state.currentChatId();
  if (!chatId) {
    return;
  }
  await api.terminalRun(chatId, line).catch((error) => showToast(String(error), "error"));
}

export async function stopMyTerminal(): Promise<void> {
  const chatId = state.currentChatId();
  if (chatId) {
    await api.terminalStop(chatId);
  }
}

export async function clearMyTerminal(): Promise<void> {
  const chatId = state.currentChatId();
  if (chatId) {
    await api.terminalClear(chatId);
    setMyTerminal((view) => ({ ...view, text: "" }));
  }
}

async function refreshTasks(chatId: string): Promise<void> {
  const listed = await api.backgroundTasks(chatId).catch(() => []);
  if (isCurrentChat(chatId)) {
    setTasks(listed);
  }
}

async function refreshArtifacts(chatId: string): Promise<void> {
  const listed = await api.chatArtifacts(chatId).catch(() => []);
  if (isCurrentChat(chatId)) {
    setArtifacts(listed);
  }
}

async function loadChatPanels(chatId: string | null): Promise<void> {
  setSelectedArtifactId(null);
  if (!chatId) {
    setTasks([]);
    setMyTerminal(EMPTY_TERMINAL);
    setPlan([]);
    setArtifacts([]);
    setChangedFiles([]);
    return;
  }
  const [listedTasks, terminal, steps, listedArtifacts] = await Promise.all([
    api.backgroundTasks(chatId).catch(() => []),
    api.terminalView(chatId).catch(() => EMPTY_TERMINAL),
    api.chatPlan(chatId).catch(() => []),
    api.chatArtifacts(chatId).catch(() => []),
  ]);
  if (!isCurrentChat(chatId)) {
    return;
  }
  setTasks(listedTasks);
  setMyTerminal(terminal);
  setPlan(steps);
  setArtifacts(listedArtifacts);
}

function restorePanelState(saved: Record<string, string>): void {
  setIsOpen(saved.panelOpen === "true");
  const savedTab = PANEL_TABS.find((known) => known.id === saved.panelTab);
  if (savedTab) {
    setTab(savedTab.id);
  }
  const savedWidth = Number(saved.panelWidth);
  if (savedWidth > 0) {
    setWidth(limitWidth(savedWidth));
  }
}

async function listenToPanelEvents(): Promise<void> {
  await listen<{ chatId: string; messageId: string; callId: string; text: string }>("command-output", (event) => {
    setLiveOutput(event.payload.callId, (previous) => appendLimited(previous, event.payload.text));
  });
  await listen<{ chatId: string }>("background-changed", (event) => {
    if (isCurrentChat(event.payload.chatId)) {
      void refreshTasks(event.payload.chatId);
    }
  });
  await listen<{ taskId: string; text: string }>("background-output", (event) => {
    if (taskOutputs[event.payload.taskId] !== undefined) {
      setTaskOutputs(event.payload.taskId, (previous) => appendLimited(previous, event.payload.text));
    }
  });
  await listen<{ chatId: string; text: string }>("terminal-output", (event) => {
    if (isCurrentChat(event.payload.chatId)) {
      setMyTerminal((view) => ({ ...view, text: appendLimited(view.text, event.payload.text), isRunning: true }));
    }
  });
  await listen<{ chatId: string; folder: string }>("terminal-folder", (event) => {
    const chatId = event.payload.chatId;
    if (isCurrentChat(chatId)) {
      void api.terminalView(chatId).then((view) => isCurrentChat(chatId) && setMyTerminal(view));
    }
  });
  await listen<BrowserView>("browser-state", (event) => setBrowser(event.payload));
  await listen<{ chatId: string; tab: PanelTab }>("panel-open", (event) => {
    if (isCurrentChat(event.payload.chatId) && !(isOpen() && tab() === event.payload.tab)) {
      openPanel(event.payload.tab);
    }
  });
  await listen("browser-download-blocked", () => showToast("Скачивание во встроенном браузере выключено"));
  await listen<{ chatId: string; steps: PlanStep[] }>("plan-changed", (event) => {
    if (isCurrentChat(event.payload.chatId)) {
      setPlan(event.payload.steps);
    }
  });
  await listen<{ chatId: string }>("artifacts-changed", (event) => {
    if (isCurrentChat(event.payload.chatId)) {
      void refreshArtifacts(event.payload.chatId);
    }
  });
}

export async function initializePanels(): Promise<void> {
  restorePanelState(state.settings());
  await listenToPanelEvents();
  setBrowser(await api.browserView().catch(() => EMPTY_BROWSER));
  createRoot(() => {
    createEffect(on(state.currentChatId, (chatId) => void loadChatPanels(chatId)));
    createEffect(on([state.currentChatId, state.usageTick, state.selectedModel], () => {
      void refreshContextUsage();
      void refreshChangedFiles();
    }));
  });
}
