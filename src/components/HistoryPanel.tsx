import { For, Show, createMemo, createSignal } from "solid-js";

import type { ChatSummary } from "../api";
import { dayGroupLabel, formatRelativeDay } from "../format";
import { showContextMenu, targetsTextEditing } from "../native";
import { canReplaceClaudeCodeChat, deleteChat, forkChat, openChat, renameChat, sendChatToClaudeCode, state, syncAllChats, syncChat, toggleHistory, togglePin } from "../store";
import { CloseIcon, EditIcon, PinIcon, SyncIcon, TrashIcon } from "./icons";
import { SourceBadge } from "./SourceBadge";

const MAX_VISIBLE_CHATS = 300;
type SourceFilter = "all" | ChatSummary["source"];

const FILTERS: { id: SourceFilter; label: string }[] = [
  { id: "all", label: "Все" },
  { id: "local", label: "Tarn" },
  { id: "claude_code", label: "Claude Code" },
  { id: "codex", label: "Codex" },
];

function ChatRow(props: { chat: ChatSummary }) {
  const [isEditing, setIsEditing] = createSignal(false);
  const [isConfirmingDelete, setIsConfirmingDelete] = createSignal(false);

  const commitRename = (title: string) => {
    setIsEditing(false);
    const trimmed = title.trim();
    if (trimmed && trimmed !== props.chat.title) {
      void renameChat(props.chat.id, trimmed);
    }
  };

  const openNativeMenu = (event: MouseEvent) => {
    if (targetsTextEditing(event)) {
      return;
    }
    const canSync = props.chat.sourcePath !== null && props.chat.id.includes(":");
    void showContextMenu([
      { text: "Открыть", run: () => void openChat(props.chat.id) },
      { text: props.chat.pinned ? "Открепить" : "Закрепить", run: () => void togglePin(props.chat) },
      { text: "Переименовать", run: () => setIsEditing(true) },
      { text: "Форк чата", run: () => void forkChat(props.chat.id, null) },
      ...(canSync ? [{ text: "Докачать чат", run: () => void syncChat(props.chat) }] : []),
      { text: "Отправить в Claude Code новым чатом", run: () => void sendChatToClaudeCode(props.chat, false) },
      ...(canReplaceClaudeCodeChat(props.chat) ? [{ text: "Заменить чат в Claude Code", run: () => void sendChatToClaudeCode(props.chat, true) }] : []),
      "separator",
      { text: "Удалить", run: () => setIsConfirmingDelete(true) },
    ]);
  };

  return (
    <div
      class="chat-row"
      classList={{ "is-active": state.currentChatId() === props.chat.id }}
      onContextMenu={openNativeMenu}
    >
      <Show
        when={!isEditing()}
        fallback={
          <input
            class="chat-rename"
            id={`rename-${props.chat.id}`}
            aria-label="Новое название"
            value={props.chat.title}
            ref={(element) => queueMicrotask(() => element.select())}
            onBlur={(event) => commitRename(event.currentTarget.value)}
            onKeyDown={(event) => {
              if (event.key === "Enter") {
                commitRename(event.currentTarget.value);
              } else if (event.key === "Escape") {
                event.preventDefault();
                setIsEditing(false);
              }
            }}
          />
        }
      >
        <button class="chat-row-open" onClick={() => void openChat(props.chat.id)} title={props.chat.title}>
          <span class="chat-row-title">{props.chat.title}</span>
          <SourceBadge source={props.chat.source} />
          <span class="chat-row-time">{formatRelativeDay(props.chat.updatedAt)}</span>
        </button>
        <span class="chat-row-actions">
          <Show
            when={!isConfirmingDelete()}
            fallback={
              <>
                <button class="danger-link" onClick={() => void deleteChat(props.chat.id)}>Удалить</button>
                <button class="action-button" aria-label="Не удалять" onClick={() => setIsConfirmingDelete(false)}><CloseIcon /></button>
              </>
            }
          >
            <button class="action-button" classList={{ "is-active": props.chat.pinned }} title={props.chat.pinned ? "Открепить" : "Закрепить"} aria-label="Закрепить" onClick={() => void togglePin(props.chat)}><PinIcon /></button>
            <button class="action-button" title="Переименовать" aria-label="Переименовать" onClick={() => setIsEditing(true)}><EditIcon /></button>
            <button class="action-button" title="Удалить" aria-label="Удалить" onClick={() => setIsConfirmingDelete(true)}><TrashIcon /></button>
          </Show>
        </span>
      </Show>
    </div>
  );
}

export function HistoryPanel() {
  const [filter, setFilter] = createSignal<SourceFilter>("all");

  const availableFilters = createMemo(() => {
    const sources = new Set(state.chats().map((chat) => chat.source));
    return FILTERS.filter((item) => item.id === "all" || sources.has(item.id as ChatSummary["source"]));
  });

  const groups = createMemo(() => {
    const selected = filter();
    const filtered = state.chats().filter((chat) => selected === "all" || chat.source === selected);
    const result: { label: string; chats: ChatSummary[] }[] = [];
    for (const chat of filtered.slice(0, MAX_VISIBLE_CHATS)) {
      const label = chat.pinned ? "Закреплённые" : dayGroupLabel(chat.updatedAt);
      const last = result[result.length - 1];
      if (last && last.label === label) {
        last.chats.push(chat);
      } else {
        result.push({ label, chats: [chat] });
      }
    }
    return result;
  });

  return (
    <>
      <div
        class="history-scrim"
        classList={{ "is-open": state.historyOpen() }}
        onClick={toggleHistory}
      />
      <aside
        class="history"
        classList={{ "is-open": state.historyOpen() }}
        aria-label="История чатов"
        aria-hidden={!state.historyOpen()}
        inert={!state.historyOpen()}
      >
        <div class="history-filters" role="group" aria-label="Источник чатов">
          <For each={availableFilters()}>
            {(item) => (
              <button class="filter-chip" classList={{ "is-active": filter() === item.id }} aria-pressed={filter() === item.id} onClick={() => setFilter(item.id)}>
                {item.label}
              </button>
            )}
          </For>
          <button class="icon-button history-sync" title="Докачать чаты" aria-label="Докачать чаты" onClick={() => void syncAllChats()}>
            <SyncIcon />
          </button>
        </div>
        <div class="history-list">
          <For each={groups()} fallback={<p class="history-empty">Чатов пока нет</p>}>
            {(group) => (
              <section class="history-group">
                <div class="section-label">{group.label}</div>
                <For each={group.chats}>{(chat) => <ChatRow chat={chat} />}</For>
              </section>
            )}
          </For>
        </div>
      </aside>
    </>
  );
}
