import { For, Show, createEffect, createMemo, createSignal, on, onCleanup, onMount } from "solid-js";
import { save } from "@tauri-apps/plugin-dialog";

import { api } from "../api";
import { formatCost, formatTokens } from "../format";
import { currentChat, forkChat, isStreaming, loadOlderMessages, showToast, state } from "../store";
import { Composer } from "./Composer";
import { CompareView } from "./CompareView";
import { panels, togglePanel } from "../panels";
import { ExportIcon, ArrowDownIcon, ForkIcon, PanelIcon } from "./icons";
import { MessageView } from "./MessageView";
import { SourceBadge } from "./SourceBadge";

const RESUME_FOLLOW_DISTANCE_PX = 24;
const LOAD_OLDER_THRESHOLD_PX = 200;
const KEYS_THAT_SCROLL_UP = ["ArrowUp", "PageUp", "Home"];

export function Chat() {
  let scrollElement: HTMLDivElement | undefined;
  let threadElement: HTMLDivElement | undefined;
  let isLoadingOlder = false;
  let isPointerDown = false;
  let previousScrollTop = 0;
  const [followsBottom, setFollowsBottom] = createSignal(true);

  const scrollToBottom = (behavior: ScrollBehavior = "auto") => {
    scrollElement?.scrollTo({ top: scrollElement.scrollHeight, behavior });
  };

  const followToBottomOnNextFrame = () => {
    requestAnimationFrame(() => {
      if (followsBottom()) {
        scrollToBottom();
      }
    });
  };

  const jumpToBottom = () => {
    setFollowsBottom(true);
    scrollToBottom("smooth");
  };

  const handleWheel = (event: WheelEvent) => {
    if (event.deltaY < 0 && !event.ctrlKey) {
      setFollowsBottom(false);
    }
  };

  const handleKeyDown = (event: KeyboardEvent) => {
    if (KEYS_THAT_SCROLL_UP.includes(event.key)) {
      setFollowsBottom(false);
    }
  };

  const totals = createMemo(() => {
    let tokens = 0;
    let cost = 0;
    let hasCost = false;
    for (const message of state.thread.messages) {
      if (message.imported) {
        continue;
      }
      tokens += (message.inputTokens ?? 0) + (message.outputTokens ?? 0);
      if (message.cost !== null) {
        cost += message.cost;
        hasCost = true;
      }
    }
    return { tokens, cost, hasCost };
  });

  const loadOlderKeepingPosition = async () => {
    if (isLoadingOlder || !scrollElement) {
      return;
    }
    isLoadingOlder = true;
    const previousHeight = scrollElement.scrollHeight;
    const previousTop = scrollElement.scrollTop;
    await loadOlderMessages();
    requestAnimationFrame(() => {
      if (scrollElement) {
        scrollElement.scrollTop = previousTop + scrollElement.scrollHeight - previousHeight;
      }
      isLoadingOlder = false;
    });
  };

  const handleScroll = () => {
    if (!scrollElement) {
      return;
    }
    const currentScrollTop = scrollElement.scrollTop;
    const movedUp = currentScrollTop < previousScrollTop;
    const movedDown = currentScrollTop > previousScrollTop;
    previousScrollTop = currentScrollTop;
    const distanceFromBottom = scrollElement.scrollHeight - currentScrollTop - scrollElement.clientHeight;
    if (movedUp && isPointerDown) {
      setFollowsBottom(false);
    } else if (movedDown && distanceFromBottom < RESUME_FOLLOW_DISTANCE_PX) {
      setFollowsBottom(true);
    }
    if (scrollElement.scrollTop < LOAD_OLDER_THRESHOLD_PX && state.hasOlder()) {
      void loadOlderKeepingPosition();
    }
  };

  createEffect(on(state.currentChatId, () => {
    setFollowsBottom(state.highlightedMessage() === null);
    followToBottomOnNextFrame();
  }));

  createEffect(on(
    () => state.thread.messages.length,
    () => {
      const lastMessage = state.thread.messages[state.thread.messages.length - 1];
      if (lastMessage?.status === "streaming") {
        setFollowsBottom(true);
        followToBottomOnNextFrame();
      }
    },
    { defer: true },
  ));

  createEffect(() => {
    const target = state.highlightedMessage();
    if (target) {
      setFollowsBottom(false);
      requestAnimationFrame(() => {
        document.getElementById(`message-${target}`)?.scrollIntoView({ block: "center" });
      });
    }
  });

  const exportCurrentChat = async () => {
    const chat = currentChat();
    if (!chat) {
      return;
    }
    const safeName = chat.title.replace(/[\\/:*?"<>|]/g, " ").slice(0, 60).trim() || "чат";
    const path = await save({
      title: "Сохранить чат",
      defaultPath: `${safeName}.md`,
      filters: [
        { name: "Markdown", extensions: ["md"] },
        { name: "JSON", extensions: ["json"] },
      ],
    });
    if (!path) {
      return;
    }
    try {
      await api.exportChat(chat.id, path);
      showToast("Чат сохранён");
    } catch (error) {
      showToast(String(error), "error");
    }
  };

  onMount(() => {
    scrollToBottom();
    const threadObserver = new ResizeObserver(followToBottomOnNextFrame);
    if (threadElement) {
      threadObserver.observe(threadElement);
    }
    onCleanup(() => threadObserver.disconnect());
  });

  return (
    <div class="chat">
      <header class="chat-header">
        <span class="chat-title" title={currentChat()?.title}>{currentChat()?.title ?? ""}</span>
        <Show when={currentChat()}>{(chat) => <SourceBadge source={chat().source} />}</Show>
        <Show when={currentChat()?.cwd}><span class="chat-cwd" title={currentChat()?.cwd ?? ""}>{currentChat()?.cwd}</span></Show>
        <span class="chat-header-spacer" />
        <Show when={totals().tokens > 0}>
          <span class="chat-stats">
            {formatTokens(totals().tokens)} ток.
            <Show when={totals().hasCost}> · {formatCost(totals().cost)}</Show>
          </span>
        </Show>
        <button
          class="icon-button"
          title="Форк чата"
          aria-label="Форк чата"
          disabled={isStreaming()}
          onClick={() => {
            const chat = currentChat();
            if (chat) {
              void forkChat(chat.id, null);
            }
          }}
        >
          <ForkIcon />
        </button>
        <button class="icon-button" title="Сохранить чат в файл" aria-label="Сохранить чат в файл" onClick={() => void exportCurrentChat()}>
          <ExportIcon />
        </button>
        <button
          class="icon-button"
          classList={{ "is-active": panels.isOpen() }}
          title="Боковая панель"
          aria-label="Боковая панель"
          aria-pressed={panels.isOpen()}
          onClick={togglePanel}
        >
          <PanelIcon />
        </button>
      </header>

      <div
        class="chat-scroll"
        ref={scrollElement}
        tabIndex={-1}
        onScroll={handleScroll}
        onWheel={handleWheel}
        onKeyDown={handleKeyDown}
        onPointerDown={() => {
          isPointerDown = true;
        }}
        onPointerUp={() => {
          isPointerDown = false;
        }}
        onPointerCancel={() => {
          isPointerDown = false;
        }}
      >
        <div ref={threadElement} class="thread" classList={{ "thread-wide": state.compareResponses.messages.length > 1 }}>
          <For each={state.thread.messages}>
            {(message, index) => (
              <Show
                when={message.compareGroup === null || message.compareGroup !== state.compareResponses.messages[0]?.compareGroup}
                fallback={<CompareView />}
              >
                <MessageView message={message} previous={state.thread.messages[index() - 1]} />
              </Show>
            )}
          </For>
        </div>
      </div>

      <div class="chat-bottom">
        <button
          class="scroll-down"
          classList={{ "is-visible": !followsBottom() }}
          aria-label="Вниз"
          tabIndex={followsBottom() ? -1 : 0}
          onClick={jumpToBottom}
        >
          <ArrowDownIcon />
        </button>
        <Composer placement="chat" />
        <Show when={isStreaming()}>
          <span class="visually-hidden" role="status">Модель отвечает</span>
        </Show>
      </div>
    </div>
  );
}
