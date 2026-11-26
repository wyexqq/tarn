import { For, Show, createEffect, createMemo, createSignal, on } from "solid-js";

import { api, type SearchHit } from "../api";
import { createModalFocus } from "../focus";
import { formatRelativeDay } from "../format";
import { createPresence } from "../motion";
import { actions, goToStart, openChat, state } from "../store";
import { SourceBadge } from "./SourceBadge";

const SEARCH_DEBOUNCE_MS = 90;
const SNIPPET_MARK_START = "\u0001";
const SNIPPET_MARK_END = "\u0002";

interface Command {
  label: string;
  hint: string;
  run: () => void;
}

interface GroupedHit {
  hit: SearchHit;
  moreInChat: number;
}

function withoutMarkdownSyntax(snippet: string): string {
  return snippet
    .replace(/\*\*|__|`+|\|/g, "")
    .replace(/(^|\n)\s*(#+\s+|>\s+|[-*]\s+|\d+\.\s+)/g, "$1")
    .replace(/\s+/g, " ")
    .trim();
}

function groupHitsByChat(hits: SearchHit[]): GroupedHit[] {
  const groups = new Map<string, GroupedHit>();
  for (const hit of hits) {
    const existing = groups.get(hit.chatId);
    if (!existing) {
      groups.set(hit.chatId, { hit, moreInChat: 0 });
    } else if (!existing.hit.snippet && hit.snippet) {
      existing.hit = hit;
    } else if (hit.snippet) {
      existing.moreInChat += 1;
    }
  }
  return [...groups.values()];
}

function Snippet(props: { text: string }) {
  const pieces = createMemo(() => withoutMarkdownSyntax(props.text).split(new RegExp(`(${SNIPPET_MARK_START}[^${SNIPPET_MARK_END}]*${SNIPPET_MARK_END})`)));
  return (
    <span class="palette-snippet">
      <For each={pieces()}>
        {(piece) => (
          <Show when={piece.startsWith(SNIPPET_MARK_START)} fallback={piece}>
            <mark>{piece.slice(1, -1)}</mark>
          </Show>
        )}
      </For>
    </span>
  );
}

export function Palette() {
  let dialogElement: HTMLDivElement | undefined;
  const presence = createPresence(state.paletteOpen);
  createModalFocus(state.paletteOpen, () => dialogElement);
  const [query, setQuery] = createSignal("");
  const [hits, setHits] = createSignal<GroupedHit[]>([]);
  const [selectedIndex, setSelectedIndex] = createSignal(0);
  let searchTimer = 0;
  let latestSearchId = 0;

  const commands: Command[] = [
    { label: "Новый чат", hint: "Ctrl+N", run: () => { actions.setPaletteOpen(false); goToStart(); } },
    { label: "Настройки и ключи", hint: "Ctrl+,", run: () => { actions.setPaletteOpen(false); actions.setSettingsTab("keys"); } },
    { label: "Импорт чатов", hint: "", run: () => { actions.setPaletteOpen(false); actions.setSettingsTab("import"); } },
    { label: "Расходы", hint: "", run: () => { actions.setPaletteOpen(false); actions.setSettingsTab("usage"); } },
  ];

  const recentHits = createMemo<GroupedHit[]>(() =>
    state.chats().slice(0, 6).map((chat) => ({
      moreInChat: 0,
      hit: {
        chatId: chat.id,
        messageId: null,
        title: chat.title,
        source: chat.source,
        snippet: null,
        updatedAt: chat.updatedAt,
      },
    })),
  );

  const isSearching = () => query().trim().length > 0;
  const visibleHits = () => (isSearching() ? hits() : recentHits());
  const visibleCommands = () => (isSearching() ? [] : commands);
  const totalRows = () => visibleHits().length + visibleCommands().length;

  const runSearch = (text: string) => {
    window.clearTimeout(searchTimer);
    setSelectedIndex(0);
    if (!text.trim()) {
      setHits([]);
      return;
    }
    const searchId = ++latestSearchId;
    searchTimer = window.setTimeout(() => {
      void api.search(text).then((results) => {
        if (searchId === latestSearchId) {
          setHits(groupHitsByChat(results));
        }
      });
    }, SEARCH_DEBOUNCE_MS);
  };

  const activateHit = (hit: SearchHit) => {
    void openChat(hit.chatId, hit.messageId);
  };

  const activateRow = (index: number) => {
    const hitCount = visibleHits().length;
    if (index < hitCount) {
      activateHit(visibleHits()[index].hit);
    } else {
      visibleCommands()[index - hitCount]?.run();
    }
  };

  const handleKeyDown = (event: KeyboardEvent) => {
    if (event.key === "ArrowDown") {
      event.preventDefault();
      setSelectedIndex((index) => Math.min(index + 1, totalRows() - 1));
    } else if (event.key === "ArrowUp") {
      event.preventDefault();
      setSelectedIndex((index) => Math.max(index - 1, 0));
    } else if (event.key === "Enter") {
      event.preventDefault();
      activateRow(selectedIndex());
    }
  };

  createEffect(on(state.paletteOpen, (isOpen) => {
    if (isOpen) {
      setQuery("");
      setHits([]);
      setSelectedIndex(0);
    }
  }));

  createEffect(on(selectedIndex, (index) => {
    document.getElementById(`palette-row-${index}`)?.scrollIntoView({ block: "nearest" });
  }, { defer: true }));

  return (
    <Show when={presence.mounted()}>
      <div class="overlay palette-overlay" classList={{ "is-closing": presence.closing() }} onMouseDown={(event) => {
        if (event.target === event.currentTarget) {
          actions.setPaletteOpen(false);
        }
      }}>
        <div class="palette" ref={dialogElement} role="dialog" aria-modal="true" aria-label="Поиск по чатам" classList={{ "is-closing": presence.closing() }}>
          <input
            data-initial-focus="true"
            id="palette-input"
            class="palette-input"
            placeholder="Поиск по всем чатам"
            aria-label="Поиск по всем чатам"
            role="combobox"
            aria-expanded="true"
            aria-controls="palette-list"
            aria-activedescendant={totalRows() > 0 ? `palette-row-${selectedIndex()}` : undefined}
            value={query()}
            onInput={(event) => {
              setQuery(event.currentTarget.value);
              runSearch(event.currentTarget.value);
            }}
            onKeyDown={handleKeyDown}
          />
          <div class="palette-list" id="palette-list" role="listbox" aria-label="Результаты">
            <Show when={!isSearching() && recentHits().length > 0}>
              <div class="section-label palette-label">Недавние</div>
            </Show>
            <For each={visibleHits()}>
              {(group, index) => (
                <button
                  id={`palette-row-${index()}`}
                  class="palette-row"
                  role="option"
                  aria-selected={selectedIndex() === index()}
                  classList={{ "is-selected": selectedIndex() === index() }}
                  onMouseMove={() => setSelectedIndex(index())}
                  onClick={() => activateHit(group.hit)}
                >
                  <span class="palette-row-main">
                    <span class="palette-title">{group.hit.title}</span>
                    <Show when={group.hit.snippet}>{(snippet) => <Snippet text={snippet()} />}</Show>
                  </span>
                  <Show when={group.moreInChat > 0}>
                    <span class="palette-more">ещё {group.moreInChat}</span>
                  </Show>
                  <SourceBadge source={group.hit.source} />
                  <span class="palette-time">{formatRelativeDay(group.hit.updatedAt)}</span>
                </button>
              )}
            </For>
            <Show when={isSearching() && hits().length === 0}>
              <div class="palette-empty">Ничего не нашлось</div>
            </Show>
            <Show when={visibleCommands().length > 0}>
              <div class="section-label palette-label">Команды</div>
            </Show>
            <For each={visibleCommands()}>
              {(command, index) => {
                const rowIndex = () => visibleHits().length + index();
                return (
                  <button
                    id={`palette-row-${rowIndex()}`}
                    class="palette-row"
                    role="option"
                    aria-selected={selectedIndex() === rowIndex()}
                    classList={{ "is-selected": selectedIndex() === rowIndex() }}
                    onMouseMove={() => setSelectedIndex(rowIndex())}
                    onClick={command.run}
                  >
                    <span class="palette-title">{command.label}</span>
                    <kbd class="palette-hint">{command.hint}</kbd>
                  </button>
                );
              }}
            </For>
          </div>
        </div>
      </div>
    </Show>
  );
}
