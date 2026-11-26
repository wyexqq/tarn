import { batch, createMemo, createSignal } from "solid-js";
import { createStore, produce, reconcile } from "solid-js/store";
import { listen } from "@tauri-apps/api/event";
import { open as openDialog } from "@tauri-apps/plugin-dialog";

import { api, type AccessMode, type Attachment, type ChatSummary, type ClaudeCodeDelivery, type ImportProgress, type Message, type MessagePart, type ModelInfo, type ProviderInfo, type RewindOutcome, type RewindPreview, type StreamPayload } from "./api";
import { withImageData } from "./images";
import { withViewTransition } from "./motion";
import { flashTaskbarIfInBackground, setNativeTheme, setTaskbarProgress, showContextMenu } from "./native";
import { notRestoredText, rewindDoneText } from "./rewindText";
import { rememberChosenTheme } from "./theme";
import { installZoomControls } from "./zoom";

const PAGE_SIZE = 60;
const TOAST_LIFETIME_MS = 4200;
const TOAST_WITH_ACTION_LIFETIME_MS = 10000;
const MAX_VISIBLE_TOASTS = 3;
const NARROW_WINDOW_WIDTH = 820;
const MAX_BUFFERED_STREAMS = 8;
const earlyStreamEvents = new Map<string, StreamPayload[]>();

export type View = "start" | "chat";
export type SettingsTab = "keys" | "access" | "skills" | "plugins" | "voice" | "memory" | "import" | "usage" | "log" | "general";

export interface ToastAction {
  text: string;
  run: () => void;
}

export interface Toast {
  id: number;
  text: string;
  tone: "info" | "error";
  action?: ToastAction;
}

export interface ComposerPrefill {
  text: string;
  attachments: Attachment[];
  onlyIfTextIs: string;
  append?: boolean;
}

const [view, setView] = createSignal<View>("start");
const [chats, setChats] = createSignal<ChatSummary[]>([]);
const [currentChatId, setCurrentChatId] = createSignal<string | null>(null);
const [thread, setThread] = createStore<{ messages: Message[] }>({ messages: [] });
const [hasOlder, setHasOlder] = createSignal(false);
const [providers, setProviders] = createSignal<ProviderInfo[]>([]);
const [models, setModels] = createSignal<ModelInfo[]>([]);
const [selectedModel, setSelectedModelSignal] = createSignal<string | null>(null);
const [compareModels, setCompareModels] = createSignal<string[]>([]);
const [settings, setSettings] = createSignal<Record<string, string>>({});
const [historyOpen, setHistoryOpen] = createSignal(false);
const [paletteOpen, setPaletteOpen] = createSignal(false);
const [settingsTab, setSettingsTab] = createSignal<SettingsTab | null>(null);
const [toasts, setToasts] = createSignal<Toast[]>([]);
const [highlightedMessage, setHighlightedMessage] = createSignal<string | null>(null);
const [importProgress, setImportProgress] = createSignal<ImportProgress | null>(null);
const [composerFocusTick, setComposerFocusTick] = createSignal(0);
const [composerPrefill, setComposerPrefill] = createSignal<ComposerPrefill | null>(null);
const [freshMessageIds, setFreshMessageIds] = createSignal<Set<string>>(new Set());
const [compareResponses, setCompareResponses] = createStore<{ messages: Message[] }>({ messages: [] });
const [usageTick, setUsageTick] = createSignal(0);
const [ready, setReady] = createSignal(false);
const [viewedImage, setViewedImage] = createSignal<string | null>(null);
const [draftAccess, setDraftAccess] = createSignal<{ mode: AccessMode; folder: string | null }>({ mode: "chat", folder: null });

let nextToastId = 1;

export const state = {
  view,
  chats,
  currentChatId,
  thread,
  hasOlder,
  providers,
  models,
  selectedModel,
  compareModels,
  settings,
  historyOpen,
  paletteOpen,
  settingsTab,
  toasts,
  highlightedMessage,
  importProgress,
  composerFocusTick,
  composerPrefill,
  freshMessageIds,
  compareResponses,
  usageTick,
  ready,
  viewedImage,
};

export const currentChat = createMemo(() => chats().find((chat) => chat.id === currentChatId()) ?? null);

export const currentAccess = createMemo<{ mode: AccessMode; folder: string | null }>(() => {
  const chat = currentChat();
  return chat ? { mode: chat.accessMode, folder: chat.cwd } : draftAccess();
});

export const selectedEffortLevels = createMemo(() => {
  const selected = selectedModel();
  const model = models().find((item) => modelRefOf(item) === selected);
  return compareModels().length > 1 ? [] : model?.effortLevels ?? [];
});

export const selectedEffort = createMemo(() => settings()[`effort.${selectedModel() ?? ""}`] ?? "");

export function chooseEffort(level: string): void {
  const selected = selectedModel();
  if (selected) {
    void updateSetting(`effort.${selected}`, level);
  }
}

export const isStreaming = createMemo(() => {
  const inThread = thread.messages.some((message) => message.status === "streaming");
  const inCompare = compareResponses.messages.some((message) => message.status === "streaming");
  return inThread || inCompare;
});

export const connectedProviders = createMemo(() => providers().filter((provider) => provider.connected));

export function modelRefOf(model: ModelInfo): string {
  return `${model.provider}/${model.id}`;
}

export function modelDisplayName(modelRef: string | null): string {
  if (!modelRef) {
    return "Выбери модель";
  }
  const found = models().find((model) => modelRefOf(model) === modelRef);
  if (found) {
    return found.name;
  }
  return modelRef.slice(modelRef.indexOf("/") + 1);
}

export function providerName(providerId: string | null): string {
  if (!providerId) {
    return "";
  }
  return providers().find((provider) => provider.id === providerId)?.name ?? providerId;
}

export function isNarrowWindow(): boolean {
  return window.innerWidth <= NARROW_WINDOW_WIDTH;
}

export function toggleHistory(): void {
  const open = !historyOpen();
  setHistoryOpen(open);
  void api.setSetting("historyOpen", String(open));
}

export function dismissToast(id: number): void {
  setToasts((current) => current.filter((toast) => toast.id !== id));
}

export function showToast(text: string, tone: Toast["tone"] = "info", action?: ToastAction): number {
  const id = nextToastId++;
  setToasts((current) => [...current, { id, text, tone, action }].slice(-MAX_VISIBLE_TOASTS));
  window.setTimeout(() => {
    setToasts((current) => current.filter((toast) => toast.id !== id));
  }, action ? TOAST_WITH_ACTION_LIFETIME_MS : TOAST_LIFETIME_MS);
  return id;
}

function reportError(error: unknown): void {
  showToast(String(error), "error");
}

export async function refreshChats(): Promise<void> {
  setChats(await api.listChats());
}

export async function refreshProviders(): Promise<void> {
  const [providerList, modelList] = await Promise.all([api.listProviders(), api.listModels()]);
  batch(() => {
    setProviders(providerList);
    setModels(modelList);
  });
  ensureSelectedModelIsAvailable(modelList);
}

function ensureSelectedModelIsAvailable(modelList: ModelInfo[]): void {
  const selected = selectedModel();
  const stillExists = selected !== null && modelList.some((model) => modelRefOf(model) === selected);
  if (!stillExists) {
    setSelectedModelSignal(modelList.length > 0 ? modelRefOf(modelList[0]) : null);
  }
}

export function selectModel(modelRef: string): void {
  setSelectedModelSignal(modelRef);
  void api.setSetting("lastModel", modelRef);
}

export function toggleCompareModel(modelRef: string): void {
  setCompareModels((current) =>
    current.includes(modelRef) ? current.filter((value) => value !== modelRef) : [...current, modelRef].slice(0, 3),
  );
}

export function clearCompareModels(): void {
  setCompareModels([]);
}

export async function updateSetting(key: string, value: string): Promise<void> {
  await api.setSetting(key, value);
  setSettings((current) => ({ ...current, [key]: value }));
}

async function askForFolder(): Promise<string | null> {
  const folder = await openDialog({ directory: true, title: "Рабочая папка" });
  return typeof folder === "string" ? folder : null;
}

function rememberAccessForNewChats(mode: AccessMode, folder: string | null): void {
  if (settings().rememberAccess === "true") {
    void updateSetting("lastAccessMode", mode);
    void updateSetting("lastAccessFolder", folder ?? "");
  }
}

async function applyAccess(mode: AccessMode, folder: string | null): Promise<void> {
  const chatId = currentChatId();
  if (chatId) {
    const updated = await api.setChatAccess(chatId, mode, folder);
    setChats((current) => current.map((chat) => (chat.id === updated.id ? updated : chat)));
  } else {
    setDraftAccess({ mode, folder });
  }
  rememberAccessForNewChats(mode, folder);
}

export async function chooseAccessMode(mode: AccessMode): Promise<void> {
  try {
    const folder = currentAccess().folder;
    if (mode === "chat" || folder) {
      await applyAccess(mode, folder);
      return;
    }
    const chosenFolder = await askForFolder();
    if (chosenFolder) {
      await applyAccess(mode, chosenFolder);
    }
  } catch (error) {
    reportError(error);
  }
}

export async function chooseAccessFolder(): Promise<void> {
  try {
    const chosenFolder = await askForFolder();
    if (chosenFolder) {
      await applyAccess(currentAccess().mode, chosenFolder);
    }
  } catch (error) {
    reportError(error);
  }
}

function restoreRememberedAccess(saved: Record<string, string>): void {
  if (saved.rememberAccess === "true" && saved.lastAccessMode) {
    setDraftAccess({ mode: saved.lastAccessMode as AccessMode, folder: saved.lastAccessFolder || null });
  } else {
    setDraftAccess({ mode: "chat", folder: null });
  }
}

export function focusComposer(): void {
  setComposerFocusTick((tick) => tick + 1);
}

export function goToStart(): void {
  withViewTransition(() => {
    batch(() => {
      setView("start");
      setCurrentChatId(null);
      setThread("messages", []);
      setCompareResponses("messages", []);
      setHasOlder(false);
      restoreRememberedAccess(settings());
    });
  });
  void refreshChats();
  focusComposer();
}

export async function openChat(chatId: string, ensureMessageId: string | null = null): Promise<void> {
  try {
    const page = await api.getThread(chatId, PAGE_SIZE, null, ensureMessageId);
    const chat = chats().find((item) => item.id === chatId);
    withViewTransition(() => {
      batch(() => {
        setCurrentChatId(chatId);
        setThread("messages", reconcile(page.messages, { key: "id" }));
        setHasOlder(page.hasMore);
        setFreshMessageIds(new Set<string>());
        setCompareResponses("messages", []);
        setView("chat");
        if (isNarrowWindow()) {
          setHistoryOpen(false);
        }
        setPaletteOpen(false);
        setHighlightedMessage(ensureMessageId);
        if (chat?.modelRef && models().some((model) => modelRefOf(model) === chat.modelRef)) {
          setSelectedModelSignal(chat.modelRef);
        }
        replayFinishedStreamsAfterReload();
      });
    });
    if (ensureMessageId) {
      window.setTimeout(() => setHighlightedMessage(null), 1800);
    }
  } catch (error) {
    reportError(error);
  }
}

export async function loadOlderMessages(): Promise<void> {
  const chatId = currentChatId();
  const oldest = thread.messages[0];
  if (!chatId || !oldest || !hasOlder()) {
    return;
  }
  const page = await api.getThread(chatId, PAGE_SIZE, oldest.id);
  batch(() => {
    setThread("messages", (existing) => [...page.messages, ...existing]);
    setHasOlder(page.hasMore);
  });
}

async function reloadThread(): Promise<void> {
  const chatId = currentChatId();
  if (!chatId) {
    return;
  }
  const page = await api.getThread(chatId, Math.max(PAGE_SIZE, thread.messages.length));
  batch(() => {
    setThread("messages", reconcile(page.messages, { key: "id" }));
    setHasOlder(page.hasMore);
    replayFinishedStreamsAfterReload();
  });
}

function markFresh(ids: string[]): void {
  setFreshMessageIds((current) => {
    const updated = new Set(current);
    for (const id of ids) {
      updated.add(id);
    }
    return updated;
  });
}

function applySendOutcome(outcome: Awaited<ReturnType<typeof api.sendMessage>>): void {
  const added = outcome.userMessage ? [outcome.userMessage, ...outcome.assistantMessages] : outcome.assistantMessages;
  markFresh(added.map((message) => message.id));
  const isCompare = outcome.assistantMessages.length > 1;
  const threadAdditions = isCompare && outcome.userMessage ? [outcome.userMessage, outcome.assistantMessages[0]] : added;
  setThread("messages", (existing) => [...existing, ...threadAdditions]);
  setCompareResponses("messages", isCompare ? outcome.assistantMessages : []);
  setCurrentChatId(outcome.chat.id);
  setChats((current) => [outcome.chat, ...current.filter((chat) => chat.id !== outcome.chat.id)]);
  replayEarlyStreamEvents();
}

export function chosenModels(): string[] {
  const compared = compareModels();
  if (compared.length > 1) {
    return compared;
  }
  const selected = selectedModel();
  return selected ? [selected] : [];
}

export async function sendMessage(text: string, attachments: Attachment[]): Promise<boolean> {
  const modelRefs = chosenModels();
  if (modelRefs.length === 0) {
    showToast("Сначала добавь ключ: Настройки → Ключи", "error");
    setSettingsTab("keys");
    return false;
  }
  try {
    const access = currentChatId() ? null : draftAccess();
    const outcome = await api.sendMessage({
      chatId: currentChatId(),
      parentId: null,
      text,
      attachments,
      modelRefs,
      accessMode: access?.mode ?? null,
      folder: access?.folder ?? null,
    });
    if (view() === "start") {
      withViewTransition(() => {
        batch(() => {
          setView("chat");
          applySendOutcome(outcome);
        });
      });
    } else {
      applySendOutcome(outcome);
    }
    return true;
  } catch (error) {
    reportError(error);
    return false;
  }
}

async function replaceTailAfterBranch(outcome: Awaited<ReturnType<typeof api.sendMessage>>, branchPointId: string): Promise<void> {
  const added = outcome.userMessage ? [outcome.userMessage, ...outcome.assistantMessages] : outcome.assistantMessages;
  markFresh(added.map((message) => message.id));
  setThread("messages", (existing) => {
    const branchIndex = existing.findIndex((message) => message.id === branchPointId);
    return branchIndex >= 0 ? [...existing.slice(0, branchIndex), ...added] : [...existing, ...added];
  });
  replayEarlyStreamEvents();
  await refreshChats();
}

export async function regenerate(message: Message, modelRef: string | null = null): Promise<void> {
  const target = modelRef ?? selectedModel();
  if (!target) {
    showToast("Сначала выбери модель", "error");
    return;
  }
  try {
    const outcome = await api.regenerateMessage(message.id, target);
    await replaceTailAfterBranch(outcome, message.id);
  } catch (error) {
    reportError(error);
  }
}

export async function editAndResend(message: Message, text: string): Promise<void> {
  const target = selectedModel();
  if (!target) {
    showToast("Сначала выбери модель", "error");
    return;
  }
  try {
    const outcome = await api.editMessage(message.id, text, target);
    await replaceTailAfterBranch(outcome, message.id);
  } catch (error) {
    reportError(error);
  }
}

export async function switchToSibling(message: Message, siblingId: string): Promise<void> {
  await api.switchBranch(message.chatId, siblingId);
  await reloadThread();
}

async function offerMessageInComposer(message: Message): Promise<void> {
  const attachments = await Promise.all(message.attachments.map(withImageData));
  setComposerPrefill({ text: message.content, attachments, onlyIfTextIs: "" });
}

export async function forkChat(chatId: string, message: Message | null): Promise<void> {
  try {
    const fork = await api.forkChat(chatId, message?.id ?? null);
    await refreshChats();
    await openChat(fork.id);
    if (message?.role === "user") {
      await offerMessageInComposer(message);
    }
    focusComposer();
  } catch (error) {
    reportError(error);
  }
}

async function undoRewind(message: Message, outcome: RewindOutcome): Promise<void> {
  if (outcome.restoredFiles > 0) {
    const notRestored = await api.undoFileRestore(message.chatId);
    if (notRestored.length > 0) {
      showToast(notRestoredText(notRestored), "error");
    }
  }
  if (!outcome.previousLeaf) {
    return;
  }
  await api.switchBranch(message.chatId, outcome.previousLeaf);
  await reloadThread();
  if (message.role === "user") {
    setComposerPrefill({ text: "", attachments: [], onlyIfTextIs: message.content });
  }
}

let lastRewindToastId: number | null = null;

async function applyRewind(message: Message, restoreFiles: boolean, preview: RewindPreview): Promise<void> {
  try {
    const outcome = await api.rewindChat(message.id, restoreFiles);
    setCompareResponses("messages", []);
    await reloadThread();
    if (message.role === "user") {
      await offerMessageInComposer(message);
    }
    focusComposer();
    if (lastRewindToastId !== null) {
      dismissToast(lastRewindToastId);
    }
    const userChoseOnlyChat = preview.changedFiles > 0 && !restoreFiles;
    const mentionCommands = preview.commandsLeftEffects && !userChoseOnlyChat;
    lastRewindToastId = showToast(rewindDoneText(outcome.restoredFiles, outcome.notRestored, mentionCommands), "info", {
      text: "Вернуть",
      run: () => void undoRewind(message, outcome).catch(reportError),
    });
  } catch (error) {
    reportError(error);
  }
}

export async function rewindTo(message: Message): Promise<void> {
  try {
    const preview = await api.rewindPreview(message.id);
    if (preview.changedFiles === 0) {
      await applyRewind(message, false, preview);
      return;
    }
    await showContextMenu([
      { text: `Чат и файлы (${preview.changedFiles})`, run: () => void applyRewind(message, true, preview) },
      { text: "Только чат", run: () => void applyRewind(message, false, preview) },
    ]);
  } catch (error) {
    reportError(error);
  }
}

export async function syncChat(chat: ChatSummary): Promise<void> {
  try {
    const newMessages = await api.syncChat(chat.id);
    await refreshChats();
    if (state.currentChatId() === chat.id) {
      await reloadThread();
    }
    showToast(newMessages > 0 ? `Докачано сообщений: ${newMessages}` : "Новых сообщений нет");
  } catch (error) {
    reportError(error);
  }
}

export async function syncAllChats(): Promise<void> {
  try {
    await api.syncAllChats();
  } catch (error) {
    reportError(error);
  }
}

export function canReplaceClaudeCodeChat(chat: ChatSummary): boolean {
  return chat.source === "claude_code" && chat.sourcePath !== null;
}

export function resumeCommandOf(delivery: ClaudeCodeDelivery): string {
  return `cd "${delivery.cwd}"; claude --resume ${delivery.sessionId}`;
}

export async function sendChatToClaudeCode(chat: ChatSummary, replaceOriginal: boolean): Promise<void> {
  try {
    const delivery = await api.sendToClaudeCode(chat.id, replaceOriginal);
    if (state.currentChatId() === chat.id) {
      await reloadThread();
    }
    if (replaceOriginal) {
      showToast("Чат в Claude Code заменён", "info", {
        text: "Вернуть",
        run: () => void api.undoClaudeCodeReplace(delivery).catch(reportError),
      });
    } else {
      const result = delivery.listedInClient
        ? "Чат добавлен в Claude. Если его нет в списке, перезапусти Claude"
        : "Чат записан в Claude Code, открыть его можно командой";
      showToast(result, "info", {
        text: "Скопировать команду",
        run: () => void navigator.clipboard.writeText(resumeCommandOf(delivery)),
      });
    }
  } catch (error) {
    reportError(error);
  }
}

export async function chooseCompareResponse(message: Message): Promise<void> {
  await api.switchBranch(message.chatId, message.id);
  setCompareResponses("messages", []);
  await reloadThread();
}

export async function stopStreaming(): Promise<void> {
  const streamingIds = [...thread.messages, ...compareResponses.messages]
    .filter((message) => message.status === "streaming")
    .map((message) => message.id);
  await Promise.all(streamingIds.map((id) => api.cancelGeneration(id)));
}

export async function deleteChat(chatId: string): Promise<void> {
  await api.deleteChat(chatId);
  if (currentChatId() === chatId) {
    goToStart();
  }
  await refreshChats();
}

export async function renameChat(chatId: string, title: string): Promise<void> {
  await api.renameChat(chatId, title);
  await refreshChats();
}

export async function togglePin(chat: ChatSummary): Promise<void> {
  await api.setChatPinned(chat.id, !chat.pinned);
  await refreshChats();
}

export function applyTheme(theme: string | undefined): void {
  setNativeTheme(theme);
  rememberChosenTheme(theme);
  if (theme === "light" || theme === "dark") {
    document.documentElement.dataset.theme = theme;
  } else {
    delete document.documentElement.dataset.theme;
  }
}

function isMessageOnScreen(messageId: string): boolean {
  const isListed = (message: Message) => message.id === messageId;
  return thread.messages.some(isListed) || compareResponses.messages.some(isListed);
}

function bufferEarlyStreamEvent(payload: StreamPayload): void {
  const events = earlyStreamEvents.get(payload.messageId) ?? [];
  events.push(payload);
  earlyStreamEvents.set(payload.messageId, events);
  if (earlyStreamEvents.size > MAX_BUFFERED_STREAMS) {
    const oldestMessageId = earlyStreamEvents.keys().next().value;
    if (oldestMessageId) {
      earlyStreamEvents.delete(oldestMessageId);
    }
  }
}

function replayEarlyStreamEvents(): void {
  for (const [messageId, events] of earlyStreamEvents) {
    if (isMessageOnScreen(messageId)) {
      earlyStreamEvents.delete(messageId);
      events.forEach(applyStreamEvent);
    }
  }
}

function replayFinishedStreamsAfterReload(): void {
  for (const [messageId, events] of earlyStreamEvents) {
    if (isMessageOnScreen(messageId)) {
      earlyStreamEvents.delete(messageId);
      events.filter((event) => event.kind === "done" || event.kind === "failed").forEach(applyStreamEvent);
    }
  }
}

function applyStreamEvent(payload: StreamPayload): void {
  if (!isMessageOnScreen(payload.messageId)) {
    bufferEarlyStreamEvent(payload);
    if (payload.kind === "done") {
      void refreshChats();
    }
    return;
  }
  const update = (list: Message[]) => {
    const target = list.find((message) => message.id === payload.messageId);
    if (!target) {
      return;
    }
    target.drafting = payload.kind === "tool" ? payload.text : undefined;
    if (payload.kind === "delta") {
      target.content += payload.text;
    } else if (payload.kind === "thinking") {
      target.thinking = (target.thinking ?? "") + payload.text;
    } else if (payload.kind === "parts") {
      target.parts = JSON.parse(payload.text) as MessagePart[];
    } else if (payload.kind === "done" && payload.message) {
      Object.assign(target, payload.message, { siblingIds: payload.message.siblingIds });
    } else if (payload.kind === "failed") {
      target.status = "error";
      target.error = payload.text;
    }
  };
  batch(() => {
    setThread("messages", produce(update));
    setCompareResponses("messages", produce(update));
  });
  if (payload.kind === "done") {
    void refreshChats();
    flashTaskbarIfInBackground();
  }
  if (payload.kind === "parts" && payload.text.includes('"status":"waiting"')) {
    flashTaskbarIfInBackground();
  }
}

function showImportProgressOnTaskbar(progress: ImportProgress): void {
  if (progress.finished || progress.filesTotal === 0) {
    setTaskbarProgress(null);
  } else {
    setTaskbarProgress(progress.filesDone / progress.filesTotal);
  }
}

export async function initializeStore(): Promise<void> {
  await listen<StreamPayload>("stream", (event) => applyStreamEvent(event.payload));
  await listen("chats-changed", () => void refreshChats());
  await listen("usage-changed", () => setUsageTick((tick) => tick + 1));
  await listen("focus-composer", () => focusComposer());
  await listen<number>("import-finished", (event) => {
    void reloadThread();
    showToast(event.payload > 0 ? `Докачано чатов: ${event.payload}` : "Новых сообщений нет");
  });
  await listen<ImportProgress>("import-progress", (event) => {
    if (event.payload.background) {
      return;
    }
    setImportProgress(event.payload);
    showImportProgressOnTaskbar(event.payload);
    if (event.payload.finished) {
      void refreshChats();
    }
  });
  const loadedSettings = await api.getSettings();
  setSettings(loadedSettings);
  applyTheme(loadedSettings.theme);
  installZoomControls(loadedSettings.zoom);
  restoreRememberedAccess(loadedSettings);
  setHistoryOpen(loadedSettings.historyOpen === "true" && !isNarrowWindow());
  await Promise.all([refreshChats(), refreshProviders()]);
  const lastModel = loadedSettings.lastModel;
  if (lastModel && models().some((model) => modelRefOf(model) === lastModel)) {
    setSelectedModelSignal(lastModel);
  }
  setReady(true);
  void api.refreshModels().then(() => refreshProviders());
}

export const actions = {
  setComposerPrefill,
  setHistoryOpen,
  setPaletteOpen,
  setSettingsTab,
  setViewedImage,
};

export function putIntoComposer(text: string): void {
  setComposerPrefill({ text, attachments: [], onlyIfTextIs: "", append: true });
}
