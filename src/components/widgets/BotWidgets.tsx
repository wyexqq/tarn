import { For, Show, createEffect, createMemo, createSignal, on, onCleanup, onMount } from "solid-js";
import { convertFileSrc } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";

import { api } from "../../api";
import { showToast } from "../../store";
import { startSimulator, stopSimulator, watchSimulator } from "../../widgets/botSim";
import { LINK_TEXT, bubbleFromMock, bubbleFromSimulator, linkStateOf, type Bubble, type BubbleButton } from "../../widgets/botView";
import { answerToPress, answerToText, startConversation, withUserText, type MockState } from "../../widgets/mockBot";
import type { BotSpec } from "../../widgets/types";
import { BotKindIcon, CopyIcon, RestartIcon } from "../icons";
import { BotChat } from "./BotChat";
import { Phone } from "./Phone";
import { WidgetAction, WidgetCard } from "./WidgetCard";

const REPLY_DELAY_MS = 420;
const SHOWN_LOG_ENTRIES = 14;
const PHOTO_EXTENSIONS = ["png", "jpg", "jpeg", "webp", "gif"];
const SCHEME = "widget";
const SIMULATOR_TITLE = "Симулятор Telegram";

function reportFailure(error: unknown): void {
  showToast(String(error), "error");
}

function MockBot(props: { spec: BotSpec; restartTick: number }) {
  const [state, setState] = createSignal<MockState>(startConversation(props.spec, Date.now()));
  const [typing, setTyping] = createSignal(false);
  const timers = new Set<number>();

  const answerLater = (answer: (current: MockState) => MockState) => {
    setTyping(true);
    const timer = window.setTimeout(() => {
      timers.delete(timer);
      setTyping(timers.size > 0);
      setState((current) => answer(current));
    }, REPLY_DELAY_MS);
    timers.add(timer);
  };

  const clearTimers = () => {
    timers.forEach((timer) => window.clearTimeout(timer));
    timers.clear();
    setTyping(false);
  };

  createEffect(
    on(
      () => props.restartTick,
      () => {
        clearTimers();
        setState(startConversation(props.spec, Date.now()));
      },
      { defer: true },
    ),
  );

  onCleanup(clearTimers);

  const send = (text: string) => {
    setState(withUserText(state(), text, Date.now()));
    answerLater((current) => answerToText(props.spec, current, text, Date.now()));
  };

  const press = (bubble: Bubble, button: BubbleButton) => {
    if (button.action.kind !== "go") {
      return;
    }
    const pressed = button.action.button;
    if (pressed.edit) {
      setState(answerToPress(props.spec, state(), bubble.id, pressed, Date.now()));
      return;
    }
    answerLater((current) => answerToPress(props.spec, current, bubble.id, pressed, Date.now()));
  };

  const keyboard = () => state().keyboard?.map((row) => row.map((button) => button.text)) ?? null;

  return (
    <div class="phone-stage">
      <Phone label={props.spec.name}>
        <BotChat name={props.spec.name} typing={typing()} bubbles={state().messages.map(bubbleFromMock)} keyboard={keyboard()} onSend={send} onPress={press} />
      </Phone>
      <div class="phone-side">
        <strong>Что делал бот</strong>
        <ul class="phone-log">
          <For each={state().log.slice(0, SHOWN_LOG_ENTRIES)} fallback={<li>пока ничего</li>}>
            {(entry) => (
              <li>
                <b>{entry.method}</b> {entry.detail}
              </li>
            )}
          </For>
        </ul>
      </div>
    </div>
  );
}

function fileAddress(fileId: string): string {
  return convertFileSrc(`file-${fileId}`, SCHEME);
}

function LiveBot(props: { startOnOpen: boolean }) {
  const view = watchSimulator();
  const [now, setNow] = createSignal(Date.now());
  const [copied, setCopied] = createSignal(false);
  const clock = window.setInterval(() => setNow(Date.now()), 1000);
  onCleanup(() => window.clearInterval(clock));

  onMount(() => {
    if (props.startOnOpen && !view()?.running) {
      startSimulator().catch(reportFailure);
    }
  });

  const running = () => Boolean(view()?.running);
  const linkState = () => linkStateOf(running(), view()?.lastCallAt ?? null, view()?.webhook ?? null, now());
  const bubbles = createMemo(() => (view()?.messages ?? []).map((message) => bubbleFromSimulator(message, fileAddress)));
  const log = createMemo(() => [...(view()?.log ?? [])].reverse().slice(0, SHOWN_LOG_ENTRIES));

  const send = (text: string) => {
    api.botSimSend(text).catch(reportFailure);
  };

  const press = (bubble: Bubble, button: BubbleButton) => {
    if (button.action.kind === "callback") {
      api.botSimPress(bubble.id, button.action.data).catch(reportFailure);
    }
  };

  const attachPhoto = async () => {
    const picked = await open({ multiple: false, directory: false, filters: [{ name: "Фото", extensions: PHOTO_EXTENSIONS }] });
    if (typeof picked === "string") {
      api.botSimSendPhoto(picked, "").catch(reportFailure);
    }
  };

  const copyAddress = async () => {
    const address = view()?.address;
    if (!address) {
      return;
    }
    await navigator.clipboard.writeText(address);
    setCopied(true);
    window.setTimeout(() => setCopied(false), 1400);
  };

  const toggleRunning = () => {
    const action = running() ? stopSimulator() : startSimulator();
    action.catch(reportFailure);
  };

  return (
    <div class="phone-stage">
      <Phone label={SIMULATOR_TITLE}>
        <BotChat
          name="Тестовый бот"
          typing={(view()?.typingUntil ?? 0) > now()}
          bubbles={bubbles()}
          keyboard={view()?.keyboard ?? null}
          notice={view()?.notice ?? null}
          commands={view()?.commands ?? []}
          disabled={!running()}
          onSend={send}
          onPress={press}
          onAttach={() => void attachPhoto()}
        />
      </Phone>
      <div class="phone-side">
        <span class="link-state" data-state={linkState()}>
          <i />
          {LINK_TEXT[linkState()]}
        </span>
        <Show when={view()?.address}>
          {(address) => (
            <div class="sim-address">
              <code>{address()}</code>
              <button class="widget-action" title={copied() ? "Скопировано" : "Скопировать адрес"} aria-label="Скопировать адрес" onClick={() => void copyAddress()}>
                <CopyIcon />
              </button>
            </div>
          )}
        </Show>
        <Show when={running()}>
          <span class="sim-hint">Этот адрес вместо api.telegram.org, токен любой</span>
        </Show>
        <div class="sim-actions">
          <button class="ghost-button" onClick={toggleRunning}>{running() ? "Остановить" : "Запустить"}</button>
          <button class="ghost-button" disabled={bubbles().length === 0} onClick={() => api.botSimReset().catch(reportFailure)}>Очистить чат</button>
        </div>
        <Show when={log().length > 0}>
          <strong>Запросы</strong>
          <ul class="phone-log">
            <For each={log()}>
              {(entry) => (
                <li classList={{ "is-problem": Boolean(entry.problem), "is-user": entry.fromUser }}>
                  <b>{entry.method}</b> {entry.detail}
                  <Show when={entry.problem}>
                    <span class="log-problem">{entry.problem}</span>
                  </Show>
                </li>
              )}
            </For>
          </ul>
        </Show>
      </div>
    </div>
  );
}

export function MockBotWidget(props: { title: string; spec: BotSpec }) {
  const [mode, setMode] = createSignal<"mock" | "live">("mock");
  const [restartTick, setRestartTick] = createSignal(0);
  return (
    <WidgetCard
      icon={<BotKindIcon />}
      title={props.title}
      actions={
        <>
          <span class="phone-tabs" role="tablist" aria-label="Режим">
            <button class="phone-tab" role="tab" aria-selected={mode() === "mock"} onClick={() => setMode("mock")}>Макет</button>
            <button class="phone-tab" role="tab" aria-selected={mode() === "live"} onClick={() => setMode("live")}>Свой бот</button>
          </span>
          <Show when={mode() === "mock"}>
            <WidgetAction label="Начать заново" onClick={() => setRestartTick(restartTick() + 1)}>
              <RestartIcon />
            </WidgetAction>
          </Show>
        </>
      }
    >
      <div hidden={mode() === "live"}>
        <MockBot spec={props.spec} restartTick={restartTick()} />
      </div>
      <Show when={mode() === "live"}>
        <LiveBot startOnOpen />
      </Show>
    </WidgetCard>
  );
}

export function LiveBotWidget() {
  return (
    <WidgetCard icon={<BotKindIcon />} title={SIMULATOR_TITLE}>
      <LiveBot startOnOpen={false} />
    </WidgetCard>
  );
}
