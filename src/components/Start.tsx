import { For, Show, createMemo, createResource } from "solid-js";

import { api } from "../api";
import { formatRelativeDay, pluralRu } from "../format";
import { actions, connectedProviders, openChat, state } from "../store";
import { Composer } from "./Composer";
import { SourceBadge } from "./SourceBadge";

const RECENT_CHAT_COUNT = 4;

export function Start() {
  const recentChats = createMemo(() => state.chats().slice(0, RECENT_CHAT_COUNT));
  const scanTrigger = () => `import-finished-${state.importProgress()?.finished ?? false}`;
  const [importSources] = createResource(scanTrigger, () => api.scanImportSources());

  const importSuggestion = createMemo(() => {
    const sources = importSources() ?? [];
    const waiting = sources.filter((source) => source.exists && source.fileCount > 0 && source.importedChats === 0);
    if (waiting.length === 0 || state.settings().importDismissed === "true") {
      return null;
    }
    const total = waiting.reduce((sum, source) => sum + source.fileCount, 0);
    const names = waiting.map((source) => (source.source === "codex" ? "Codex" : "Claude Code")).join(" и ");
    return `Нашёл ${total} ${pluralRu(total, "чат", "чата", "чатов")} ${names}`;
  });

  return (
    <div class="start">
      <h1 class="start-title">С чего начнём?</h1>
      <Composer placement="start" />

      <Show when={recentChats().length > 0}>
        <section class="resume" aria-label="Недавние чаты">
          <div class="section-label">Продолжить</div>
          <For each={recentChats()}>
            {(chat, index) => (
              <button class="resume-row" style={{ "--stagger": index() }} onClick={() => void openChat(chat.id)}>
                <span class="resume-title">{chat.title}</span>
                <SourceBadge source={chat.source} />
                <span class="resume-time">{formatRelativeDay(chat.updatedAt)}</span>
              </button>
            )}
          </For>
        </section>
      </Show>

      <div class="start-footer">
        <Show when={importSuggestion()}>
          {(suggestion) => (
            <button class="import-hint" onClick={() => actions.setSettingsTab("import")}>
              {suggestion()} · Импортировать
            </button>
          )}
        </Show>
        <div class="keys-row">
          <For each={connectedProviders()}>
            {(provider) => (
              <span class="key-chip">
                <i class="status-dot" />
                {provider.name}
              </span>
            )}
          </For>
          <button class="key-add" onClick={() => actions.setSettingsTab("keys")}>
            {connectedProviders().length === 0 ? "Добавить ключ" : "+ ключ"}
          </button>
        </div>
      </div>
    </div>
  );
}
