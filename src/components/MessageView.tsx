import { For, Index, Show, createEffect, createMemo, createSignal, onCleanup } from "solid-js";
import { openUrl } from "@tauri-apps/plugin-opener";

import type { Artifact, Attachment, Message, ToolCallPart } from "../api";
import { formatCost, formatFullMoment, formatMessageTime, formatTokens, pluralRu, shortModelName } from "../format";
import { renderMarkdown } from "../markdown";
import { PANEL_TABS, describeArtifact, panelOfTool, showArtifact, showToolInPanel } from "../panels";
import { showContextMenu, targetsTextEditing } from "../native";
import { editAndResend, forkChat, isStreaming, modelDisplayName, regenerate, rewindTo, showToast, state, switchToSibling } from "../store";
import { api, type ToolAnswer } from "../api";
import { CheckIcon, ChevronIcon, CopyIcon, EditIcon, FileIcon, ForkIcon, RetryIcon, RewindIcon, ToolIcon } from "./icons";
import { ImageView } from "./ImageView";
import { InlineWidget } from "./widgets/WidgetView";
import { inlineParts, isHiddenFromActions, splitByWidgets, type Piece } from "../widgets/placement";

const SOURCE_NAMES = { claude_code: "Claude Code", codex: "Codex", local: "" } as const;

function toolSummary(part: ToolCallPart): string {
  try {
    const parsed = JSON.parse(part.input) as Record<string, unknown>;
    const firstValue = Object.values(parsed).find((value) => typeof value === "string");
    if (typeof firstValue === "string") {
      return firstValue.split("\n")[0];
    }
  } catch {
    return part.input.split("\n")[0];
  }
  return "";
}

function ToolCalls(props: { parts: ToolCallPart[] }) {
  const [isOpen, setIsOpen] = createSignal(false);
  const failedCount = createMemo(() => props.parts.filter((part) => part.isError).length);
  return (
    <div class="tool-group" classList={{ "is-open": isOpen() }}>
      <button class="tool-summary" aria-expanded={isOpen()} onClick={() => setIsOpen(!isOpen())}>
        <span class="tool-chevron"><ChevronIcon /></span>
        <ToolIcon />
        <span>
          {props.parts.length} {pluralRu(props.parts.length, "действие", "действия", "действий")}
        </span>
        <Show when={failedCount() > 0}>
          <span class="tool-failed">ошибок: {failedCount()}</span>
        </Show>
      </button>
      <div class="tool-body">
        <div class="tool-body-inner">
          <Show when={isOpen()}>
            <For each={props.parts}>{(part) => <ToolCallRow part={part} />}</For>
          </Show>
        </div>
      </div>
    </div>
  );
}

function parseToolInput(input: string): Record<string, unknown> | null {
  try {
    return JSON.parse(input) as Record<string, unknown>;
  } catch {
    return null;
  }
}

function DiffLines(props: { text: string; kind: "added" | "removed" }) {
  const marker = () => (props.kind === "added" ? "+ " : "- ");
  return (
    <For each={props.text.split("\n")}>
      {(line) => <span class={`diff-line diff-${props.kind}`}>{marker()}{line}</span>}
    </For>
  );
}

function ToolInput(props: { input: string }) {
  const parsed = createMemo(() => parseToolInput(props.input));
  const oldText = () => parsed()?.old_string;
  const newText = () => parsed()?.new_string;
  const isDiff = () => typeof oldText() === "string" && typeof newText() === "string";
  const plainText = () => {
    const value = parsed();
    if (value === null) {
      return props.input;
    }
    if (typeof value.command === "string") {
      return value.command;
    }
    if (typeof value.path === "string" && typeof value.content === "string") {
      return `${value.path}\n\n${value.content}`;
    }
    return JSON.stringify(value, null, 2);
  };
  return (
    <Show when={isDiff()} fallback={<pre class="tool-detail">{plainText()}</pre>}>
      <pre class="tool-detail tool-diff">
        <DiffLines text={oldText() as string} kind="removed" />
        <DiffLines text={newText() as string} kind="added" />
      </pre>
    </Show>
  );
}

function ToolCallRow(props: { part: ToolCallPart }) {
  const [isOpen, setIsOpen] = createSignal(false);
  const panelTitle = () => PANEL_TABS.find((tab) => tab.id === panelOfTool(props.part.name))?.title ?? null;
  return (
    <div class="tool-row">
      <div class="tool-row-line">
        <button class="tool-row-head" aria-expanded={isOpen()} onClick={() => setIsOpen(!isOpen())}>
          <span class="tool-name" classList={{ "is-error": props.part.isError }}>{props.part.name}</span>
          <span class="tool-preview">{toolSummary(props.part)}</span>
        </button>
        <Show when={panelTitle()}>
          {(title) => (
            <button class="tool-panel-link" onClick={() => showToolInPanel(props.part.name)}>{title()}</button>
          )}
        </Show>
      </div>
      <Show when={isOpen()}>
        <ToolInput input={props.part.input} />
        <Show when={props.part.result}>
          <pre class="tool-detail tool-detail-result">{props.part.result}</pre>
        </Show>
        <Show when={props.part.images?.length}>
          <div class="tool-images">
            <For each={props.part.images}>{(image) => <ImageView attachment={image} />}</For>
          </div>
        </Show>
      </Show>
    </div>
  );
}

const TOOL_ACTION_NAMES: Record<string, string> = {
  read_file: "Читает файл",
  list_files: "Смотрит папку",
  search_text: "Ищет в файлах",
  write_file: "Записывает файл",
  edit_file: "Правит файл",
  run_command: "Выполняет команду",
  read_skill: "Читает скилл",
  remember: "Запоминает",
  forget: "Удаляет из памяти",
  read_memory_note: "Читает заметку памяти",
  start_background: "Запускает в фоне",
  read_background: "Смотрит фоновую задачу",
  stop_background: "Останавливает фоновую задачу",
  browser_open: "Открывает страницу",
  browser_search: "Ищет в интернете",
  browser_read: "Читает страницу",
  browser_click: "Нажимает на странице",
  browser_type: "Печатает на странице",
  update_plan: "Обновляет план",
  create_artifact: "Создаёт артефакт",
  send_file: "Отправляет файл",
  show_widget: "Рисует виджет",
  bot_simulator: "Проверяет бота",
};

function actionNameFor(toolName: string): string {
  return TOOL_ACTION_NAMES[toolName] ?? `Вызывает ${toolName}`;
}

function actionNameOf(part: ToolCallPart): string {
  return actionNameFor(part.name);
}

function approvalQuestion(part: ToolCallPart): string {
  const folder = part.folder ?? "";
  if (part.name === "remember" || part.name === "forget") {
    return "Модель хочет изменить свою память";
  }
  if (part.risk === "external:browser") {
    return "Модель хочет действовать во встроенном браузере";
  }
  if (part.risk === "external:simulator") {
    return "Модель хочет написать боту в симуляторе Telegram";
  }
  if (part.risk === "command") {
    return `Модель хочет выполнить команду в папке ${folder}`;
  }
  if (part.risk === "edit") {
    return `Модель хочет изменить файл в папке ${folder}`;
  }
  return `Модель хочет вызвать инструмент ${part.name}`;
}

function ApprovalCard(props: { messageId: string; part: ToolCallPart }) {
  const [isAnswered, setIsAnswered] = createSignal(false);
  const answer = async (choice: ToolAnswer) => {
    setIsAnswered(true);
    try {
      await api.answerToolRequest(props.messageId, props.part.id ?? "", choice);
    } catch (error) {
      showToast(String(error), "error");
    }
  };
  return (
    <div class="approval-card" role="group" aria-label="Запрос разрешения">
      <p class="approval-question">{approvalQuestion(props.part)}</p>
      <ToolInput input={props.part.input} />
      <div class="approval-actions">
        <button class="primary-button" disabled={isAnswered()} onClick={() => void answer("once")}>Разрешить</button>
        <button class="ghost-button" disabled={isAnswered()} onClick={() => void answer("chat")}>Разрешить в этом чате</button>
        <button class="ghost-button" disabled={isAnswered()} onClick={() => void answer("deny")}>Запретить</button>
      </div>
    </div>
  );
}

function CopyButton(props: { text: string }) {
  const [copied, setCopied] = createSignal(false);
  const copy = async () => {
    await navigator.clipboard.writeText(props.text);
    setCopied(true);
    window.setTimeout(() => setCopied(false), 1400);
  };
  return (
    <button class="action-button" classList={{ "is-copied": copied() }} title="Скопировать" aria-label="Скопировать" onClick={() => void copy()}>
      <Show when={copied()} fallback={<CopyIcon />}>
        <CheckIcon />
      </Show>
    </button>
  );
}

function MessageTime(props: { message: Message }) {
  return (
    <Show when={props.message.createdAt > 0}>
      <span class="message-time" title={formatFullMoment(props.message.createdAt)}>{formatMessageTime(props.message.createdAt)}</span>
    </Show>
  );
}

function ForkAndRewindButtons(props: { message: Message }) {
  return (
    <>
      <button class="action-button" title="Форк отсюда" aria-label="Форк отсюда" disabled={isStreaming()} onClick={() => void forkChat(props.message.chatId, props.message)}>
        <ForkIcon />
      </button>
      <button class="action-button" title="Откатить сюда" aria-label="Откатить сюда" disabled={isStreaming()} onClick={() => void rewindTo(props.message)}>
        <RewindIcon />
      </button>
    </>
  );
}

function BranchSwitcher(props: { message: Message }) {
  const position = createMemo(() => props.message.siblingIds.indexOf(props.message.id));
  const go = (offset: number) => {
    const target = props.message.siblingIds[position() + offset];
    if (target) {
      void switchToSibling(props.message, target);
    }
  };
  return (
    <span class="branch-switcher">
      <button aria-label="Предыдущий вариант" disabled={position() <= 0} onClick={() => go(-1)}>‹</button>
      <span>{position() + 1}/{props.message.siblingIds.length}</span>
      <button aria-label="Следующий вариант" disabled={position() >= props.message.siblingIds.length - 1} onClick={() => go(1)}>›</button>
    </span>
  );
}

const MIN_STREAM_RENDER_INTERVAL_MS = 33;
const RENDER_COST_MULTIPLIER = 4;

function MarkdownBody(props: { source: string; streaming: boolean }) {
  const [html, setHtml] = createSignal(renderMarkdown(props.source));
  let renderTimer = 0;
  let lastRenderCost = 0;

  const renderNow = () => {
    renderTimer = 0;
    const startedAt = performance.now();
    setHtml(renderMarkdown(props.source));
    lastRenderCost = performance.now() - startedAt;
  };

  createEffect(() => {
    props.source;
    if (!props.streaming) {
      window.clearTimeout(renderTimer);
      renderNow();
    } else if (renderTimer === 0) {
      const delay = Math.max(MIN_STREAM_RENDER_INTERVAL_MS, lastRenderCost * RENDER_COST_MULTIPLIER);
      renderTimer = window.setTimeout(renderNow, delay);
    }
  });

  onCleanup(() => window.clearTimeout(renderTimer));

  const handleClick = (event: MouseEvent) => {
    const target = event.target as HTMLElement;
    const link = target.closest<HTMLAnchorElement>("[data-external-link]");
    if (link) {
      event.preventDefault();
      void openUrl(link.href);
      return;
    }
    const copyButton = target.closest<HTMLButtonElement>("[data-copy-code]");
    if (copyButton) {
      const code = copyButton.closest(".code-block")?.querySelector("code")?.textContent ?? "";
      void navigator.clipboard.writeText(code).then(() => {
        copyButton.textContent = "Скопировано";
        window.setTimeout(() => {
          copyButton.textContent = "Скопировать";
        }, 1400);
      });
    }
  };

  return <div class="markdown" innerHTML={html()} onClick={handleClick} />;
}

function AttachmentView(props: { attachment: Attachment }) {
  return (
    <Show
      when={props.attachment.kind === "image"}
      fallback={
        <span class="attachment-chip">
          <FileIcon />
          <span class="attachment-name">{props.attachment.name}</span>
        </span>
      }
    >
      <ImageView attachment={props.attachment} />
    </Show>
  );
}

function ThinkingBlock(props: { text: string; active: boolean }) {
  const [isOpen, setIsOpen] = createSignal(false);
  return (
    <div class="thinking" classList={{ "is-open": isOpen() }}>
      <button class="thinking-toggle" aria-expanded={isOpen()} onClick={() => setIsOpen(!isOpen())}>
        <span class="tool-chevron"><ChevronIcon /></span>
        <span classList={{ shimmer: props.active }}>{props.active ? "Думает" : "Ход мыслей"}</span>
      </button>
      <Show when={isOpen()}>
        <div class="thinking-text">{props.text}</div>
      </Show>
    </div>
  );
}

export function UserMessage(props: { message: Message }) {
  const [isEditing, setIsEditing] = createSignal(false);
  const [draft, setDraft] = createSignal("");

  const beginEditing = () => {
    setDraft(props.message.content);
    setIsEditing(true);
  };

  const submitEdit = () => {
    const text = draft().trim();
    if (!text) {
      showToast("Сообщение пустое", "error");
      return;
    }
    setIsEditing(false);
    void editAndResend(props.message, text);
  };

  return (
    <div class="user-block">
      <Show
        when={isEditing()}
        fallback={
          <>
            <Show when={props.message.attachments.length > 0}>
              <div class="message-attachments">
                <For each={props.message.attachments}>{(attachment) => <AttachmentView attachment={attachment} />}</For>
              </div>
            </Show>
            <Show when={props.message.content}>
              <div class="user-bubble">{props.message.content}</div>
            </Show>
            <div class="message-actions user-actions">
              <Show when={props.message.siblingIds.length > 1}>
                <BranchSwitcher message={props.message} />
              </Show>
              <CopyButton text={props.message.content} />
              <button class="action-button" title="Изменить" aria-label="Изменить сообщение" onClick={beginEditing}>
                <EditIcon />
              </button>
              <ForkAndRewindButtons message={props.message} />
              <MessageTime message={props.message} />
            </div>
          </>
        }
      >
        <div class="edit-box">
          <textarea
            id={`edit-${props.message.id}`}
            class="edit-input"
            value={draft()}
            onInput={(event) => setDraft(event.currentTarget.value)}
            onKeyDown={(event) => {
              if (event.key === "Enter" && !event.shiftKey) {
                event.preventDefault();
                submitEdit();
              } else if (event.key === "Escape") {
                event.preventDefault();
                setIsEditing(false);
              }
            }}
            ref={(element) => queueMicrotask(() => element.focus())}
          />
          <div class="edit-actions">
            <button class="ghost-button" onClick={() => setIsEditing(false)}>Отмена</button>
            <button class="primary-button" onClick={submitEdit}>Создать ветку</button>
          </div>
        </div>
      </Show>
    </div>
  );
}

function textOfPiece(piece: Piece): string {
  return piece.kind === "text" ? piece.text : "";
}

function partOfPiece(piece: Piece): ToolCallPart | null {
  return piece.kind === "widget" ? piece.part : null;
}

function AnswerWithWidgets(props: { content: string; widgets: ToolCallPart[]; streaming: boolean }) {
  const pieces = createMemo(() => splitByWidgets(props.content, props.widgets));
  return (
    <Index each={pieces()}>
      {(piece) => (
        <Show when={partOfPiece(piece())} fallback={<Show when={textOfPiece(piece()).trim()}><MarkdownBody source={textOfPiece(piece())} streaming={props.streaming} /></Show>}>
          {(part) => <InlineWidget part={part()} />}
        </Show>
      )}
    </Index>
  );
}

function ArtifactCard(props: { artifact: Artifact }) {
  return (
    <button class="file-card" onClick={() => showArtifact(props.artifact.id)}>
      <FileIcon />
      <span class="file-card-main">
        <span class="file-card-name">{props.artifact.title}</span>
        <span class="file-card-meta">{describeArtifact(props.artifact)}</span>
      </span>
    </button>
  );
}

export function AssistantMessage(props: { message: Message; compact?: boolean }) {
  const toolParts = createMemo(() => props.message.parts.filter((part): part is ToolCallPart => part.type === "tool_call"));
  const actionParts = createMemo(() => toolParts().filter((part) => !isHiddenFromActions(part)));
  const widgetParts = createMemo(() => inlineParts(props.message.parts));
  const draftingName = () => (isStreamingNow() && !runningPart() && !waitingPart() ? props.message.drafting : undefined);
  const artifactsInMessage = createMemo(() => toolParts().flatMap((part) => (part.artifact ? [part.artifact] : [])));
  const isStreamingNow = () => props.message.status === "streaming";
  const waitingPart = createMemo(() => (isStreamingNow() ? toolParts().find((part) => part.status === "waiting") : undefined));
  const runningPart = createMemo(() => (isStreamingNow() ? toolParts().find((part) => part.status === "running") : undefined));
  const isWaiting = () => isStreamingNow() && props.message.content.length === 0 && !props.message.thinking && !waitingPart() && !runningPart() && !draftingName();

  const metaLine = createMemo(() => {
    const message = props.message;
    const pieces: string[] = [];
    if (message.model) {
      const reference = message.provider ? `${message.provider}/${message.model}` : null;
      pieces.push(reference && state.models().length > 0 ? modelDisplayName(reference) : shortModelName(message.model));
    }
    if (message.outputTokens !== null && !message.imported) {
      pieces.push(`${formatTokens((message.inputTokens ?? 0) + message.outputTokens)} ток.`);
    }
    if (message.cost !== null) {
      pieces.push(formatCost(message.cost));
    }
    return pieces.join(" · ");
  });

  return (
    <div class="assistant-block">
      <Show when={props.message.thinking}>
        {(text) => <ThinkingBlock text={text()} active={isStreamingNow() && props.message.content.length === 0} />}
      </Show>
      <Show when={actionParts().length > 0}>
        <ToolCalls parts={actionParts()} />
      </Show>
      <Show when={isWaiting()}>
        <div class="waiting" aria-label="Модель думает"><span class="shimmer">Думает</span></div>
      </Show>
      <Show
        when={widgetParts().length > 0}
        fallback={
          <Show when={props.message.content}>
            <MarkdownBody source={props.message.content} streaming={isStreamingNow()} />
          </Show>
        }
      >
        <AnswerWithWidgets content={props.message.content} widgets={widgetParts()} streaming={isStreamingNow()} />
      </Show>
      <Show when={artifactsInMessage().length > 0}>
        <div class="file-cards">
          <For each={artifactsInMessage()}>{(artifact) => <ArtifactCard artifact={artifact} />}</For>
        </div>
      </Show>
      <Show when={runningPart()} keyed>
        {(part) => (
          <div class="tool-activity">
            <span class="shimmer">{actionNameOf(part)}</span>
            <span class="tool-preview">{toolSummary(part)}</span>
          </div>
        )}
      </Show>
      <Show when={draftingName()}>
        {(name) => (
          <div class="tool-activity">
            <span class="shimmer">{actionNameFor(name())}</span>
          </div>
        )}
      </Show>
      <Show when={waitingPart()} keyed>
        {(part) => <ApprovalCard messageId={props.message.id} part={part} />}
      </Show>
      <Show when={props.message.note}>
        <div class="message-note">{props.message.note}</div>
      </Show>
      <Show when={props.message.error}>
        <div class="message-error" role="alert">
          <span>{props.message.error}</span>
          <button class="ghost-button" onClick={() => void regenerate(props.message)}>Повторить</button>
        </div>
      </Show>
      <Show when={!isStreamingNow() && !props.compact}>
        <div class="message-actions">
          <MessageTime message={props.message} />
          <Show when={props.message.siblingIds.length > 1}>
            <BranchSwitcher message={props.message} />
          </Show>
          <Show when={props.message.content}>
            <CopyButton text={props.message.content} />
          </Show>
          <button class="action-button" title="Ответить заново" aria-label="Ответить заново" onClick={() => void regenerate(props.message)}>
            <RetryIcon />
          </button>
          <ForkAndRewindButtons message={props.message} />
          <span class="message-meta">{metaLine()}</span>
        </div>
      </Show>
    </div>
  );
}

function needsHandoffDivider(message: Message, previous: Message | undefined): boolean {
  return previous !== undefined && previous.imported && !message.imported;
}

export function MessageView(props: { message: Message; previous: Message | undefined }) {
  const isFresh = () => state.freshMessageIds().has(props.message.id);
  const openNativeMenu = (event: MouseEvent) => {
    if (targetsTextEditing(event)) {
      return;
    }
    const copyEntry = { text: "Скопировать сообщение", run: () => void navigator.clipboard.writeText(props.message.content) };
    const branchEntries = isStreaming()
      ? []
      : [
          { text: "Форк отсюда", run: () => void forkChat(props.message.chatId, props.message) },
          { text: "Откатить сюда", run: () => void rewindTo(props.message) },
        ];
    if (props.message.role === "assistant" && props.message.status !== "streaming") {
      void showContextMenu([copyEntry, { text: "Ответить заново", run: () => void regenerate(props.message) }, ...branchEntries]);
    } else {
      void showContextMenu([copyEntry, ...branchEntries]);
    }
  };
  const sourceName = () => {
    const chat = state.chats().find((item) => item.id === props.message.chatId);
    return chat ? SOURCE_NAMES[chat.source] : "";
  };
  return (
    <>
      <Show when={needsHandoffDivider(props.message, props.previous)}>
        <div class="handoff">Выше история из {sourceName()}</div>
      </Show>
      <article
        id={`message-${props.message.id}`}
        class={`message message-${props.message.role}`}
        onContextMenu={openNativeMenu}
        classList={{
          "is-fresh": isFresh(),
          "is-highlighted": state.highlightedMessage() === props.message.id,
        }}
      >
        <Show when={props.message.role === "user"} fallback={<AssistantMessage message={props.message} />}>
          <UserMessage message={props.message} />
        </Show>
      </article>
    </>
  );
}
