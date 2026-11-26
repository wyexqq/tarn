import { For, Index, Show, createEffect, createSignal, on, onCleanup } from "solid-js";
import { openUrl } from "@tauri-apps/plugin-opener";

import type { TextRun } from "../../api";
import { clockOf, type Bubble, type BubbleButton } from "../../widgets/botView";
import { ArrowUpIcon, KeyboardIcon, MenuLinesIcon, PaperclipIcon } from "../icons";

const NOTICE_LIFETIME_MS = 2600;

export interface ChatNotice {
  serial: number;
  text: string;
  alert: boolean;
}

export interface ChatCommand {
  command: string;
  description: string;
}

function openWebAddress(address: string): void {
  if (/^https?:\/\//i.test(address)) {
    void openUrl(address);
  }
}

function RunView(props: { run: TextRun }) {
  const [revealed, setRevealed] = createSignal(false);
  const classes = () => ({
    "run-bold": Boolean(props.run.bold),
    "run-italic": Boolean(props.run.italic),
    "run-underline": Boolean(props.run.underline),
    "run-strike": Boolean(props.run.strike),
    "run-code": Boolean(props.run.code),
    "run-quote": Boolean(props.run.quote),
    "run-spoiler": Boolean(props.run.spoiler) && !revealed(),
  });
  return (
    <Show
      when={props.run.link}
      fallback={
        <span classList={classes()} onClick={() => setRevealed(true)}>
          {props.run.text}
        </span>
      }
    >
      {(link) => (
        <a
          class="run-link"
          classList={classes()}
          href={link()}
          onClick={(event) => {
            event.preventDefault();
            openWebAddress(link());
          }}
        >
          {props.run.text}
        </a>
      )}
    </Show>
  );
}

function BubbleView(props: { bubble: Bubble; onPress: (bubble: Bubble, button: BubbleButton) => void }) {
  const hasText = () => props.bubble.runs.some((run) => run.text.length > 0);
  return (
    <>
      <div class="msg" classList={{ "from-bot": props.bubble.fromBot, "from-me": !props.bubble.fromBot }}>
        <Show when={props.bubble.forwarded}>
          <span class="msg-forwarded">Пересланное сообщение</span>
        </Show>
        <Show when={props.bubble.picture}>
          {(picture) => (
            <Show
              when={picture().source}
              fallback={<div class="msg-media" classList={{ "is-photo": picture().isPhoto, "is-alone": !hasText() }}>{picture().caption}</div>}
            >
              {(source) => <img class="msg-photo" classList={{ "is-alone": !hasText() }} src={source()} alt={picture().caption} draggable={false} />}
            </Show>
          )}
        </Show>
        <Show when={hasText()}>
          <span class="msg-text">
            <Index each={props.bubble.runs}>{(run) => <RunView run={run()} />}</Index>
          </span>
        </Show>
        <time>
          <Show when={props.bubble.edited}>изменено </Show>
          {clockOf(props.bubble.date)}
        </time>
      </div>
      <Show when={props.bubble.buttons.length > 0}>
        <div class="inline-keys" classList={{ "is-mine": !props.bubble.fromBot }}>
          <For each={props.bubble.buttons}>
            {(row) => (
              <div class="inline-row">
                <For each={row}>
                  {(button) => (
                    <button class="inline-key" classList={{ "is-link": button.action.kind === "url" }} onClick={() => props.onPress(props.bubble, button)}>
                      {button.text}
                    </button>
                  )}
                </For>
              </div>
            )}
          </For>
        </div>
      </Show>
    </>
  );
}

export function BotChat(props: {
  name: string;
  typing: boolean;
  bubbles: Bubble[];
  keyboard: string[][] | null;
  notice?: ChatNotice | null;
  commands?: ChatCommand[];
  disabled?: boolean;
  onSend: (text: string) => void;
  onPress: (bubble: Bubble, button: BubbleButton) => void;
  onAttach?: () => void;
}) {
  const [draft, setDraft] = createSignal("");
  const [keyboardOpen, setKeyboardOpen] = createSignal(true);
  const [menuOpen, setMenuOpen] = createSignal(false);
  const [shownNotice, setShownNotice] = createSignal<ChatNotice | null>(null);
  let feed: HTMLDivElement | undefined;
  let noticeTimer = 0;

  const commands = () => props.commands ?? [];
  const lastSignature = () => {
    const last = props.bubbles[props.bubbles.length - 1];
    return last ? `${last.id}:${last.edited}:${last.runs.length}` : "";
  };

  createEffect(
    on([() => props.bubbles.length, lastSignature, () => props.typing], () => {
      requestAnimationFrame(() => feed?.scrollTo({ top: feed.scrollHeight, behavior: "smooth" }));
    }),
  );

  createEffect(
    on(
      () => props.notice?.serial,
      () => {
        const notice = props.notice ?? null;
        window.clearTimeout(noticeTimer);
        setShownNotice(notice);
        if (notice && !notice.alert) {
          noticeTimer = window.setTimeout(() => setShownNotice(null), NOTICE_LIFETIME_MS);
        }
      },
      { defer: true },
    ),
  );

  createEffect(on(() => props.keyboard, () => setKeyboardOpen(true), { defer: true }));

  onCleanup(() => window.clearTimeout(noticeTimer));

  const send = (text: string) => {
    const trimmed = text.trim();
    if (!trimmed || props.disabled) {
      return;
    }
    props.onSend(trimmed);
    setMenuOpen(false);
  };

  const submit = (event: SubmitEvent) => {
    event.preventDefault();
    send(draft());
    setDraft("");
  };

  const press = (bubble: Bubble, button: BubbleButton) => {
    if (button.action.kind === "url") {
      openWebAddress(button.action.url);
      return;
    }
    if (!props.disabled) {
      props.onPress(bubble, button);
    }
  };

  return (
    <div class="chat-screen">
      <div class="chat-top">
        <span class="chat-avatar" aria-hidden="true">{props.name.trim().charAt(0).toUpperCase() || "Б"}</span>
        <span class="chat-name">
          <strong>{props.name}</strong>
          <small>{props.typing ? "печатает…" : "бот"}</small>
        </span>
      </div>
      <div class="chat-feed" ref={feed} aria-live="polite">
        <Index each={props.bubbles}>{(bubble) => <BubbleView bubble={bubble()} onPress={press} />}</Index>
        <Show when={props.typing}>
          <div class="typing" aria-label="Бот печатает">
            <i />
            <i />
            <i />
          </div>
        </Show>
      </div>
      <Show when={shownNotice()}>
        {(notice) => (
          <div class="chat-notice" classList={{ "is-alert": notice().alert }} role="status">
            <span>{notice().text}</span>
            <Show when={notice().alert}>
              <button class="chat-notice-ok" onClick={() => setShownNotice(null)}>OK</button>
            </Show>
          </div>
        )}
      </Show>
      <Show when={menuOpen() && commands().length > 0}>
        <ul class="chat-commands">
          <For each={commands()}>
            {(item) => (
              <li>
                <button onClick={() => send(`/${item.command}`)}>
                  <strong>/{item.command}</strong>
                  <span>{item.description}</span>
                </button>
              </li>
            )}
          </For>
        </ul>
      </Show>
      <form class="chat-input" onSubmit={submit}>
        <Show when={commands().length > 0}>
          <button type="button" class="chat-tool" aria-label="Команды бота" aria-expanded={menuOpen()} onClick={() => setMenuOpen(!menuOpen())}>
            <MenuLinesIcon />
          </button>
        </Show>
        <Show when={props.onAttach}>
          {(attach) => (
            <button type="button" class="chat-tool" aria-label="Отправить фото" disabled={props.disabled} onClick={() => attach()()}>
              <PaperclipIcon />
            </button>
          )}
        </Show>
        <input
          value={draft()}
          placeholder="Сообщение"
          autocomplete="off"
          aria-label={`Сообщение для ${props.name}`}
          disabled={props.disabled}
          onInput={(event) => setDraft(event.currentTarget.value)}
        />
        <Show when={props.keyboard}>
          <button type="button" class="chat-tool" aria-label="Клавиатура бота" aria-pressed={keyboardOpen()} onClick={() => setKeyboardOpen(!keyboardOpen())}>
            <KeyboardIcon />
          </button>
        </Show>
        <button class="chat-send" type="submit" aria-label="Отправить" disabled={props.disabled || !draft().trim()}>
          <ArrowUpIcon />
        </button>
      </form>
      <Show when={props.keyboard && keyboardOpen()}>
        <div class="reply-keyboard">
          <For each={props.keyboard ?? []}>
            {(row) => (
              <div class="reply-row">
                <For each={row}>
                  {(label) => (
                    <button class="reply-key" disabled={props.disabled} onClick={() => send(label)}>
                      {label}
                    </button>
                  )}
                </For>
              </div>
            )}
          </For>
        </div>
      </Show>
    </div>
  );
}
