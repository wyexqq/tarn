import { Show, createEffect, createSignal, on, onCleanup, untrack } from "solid-js";
import { convertFileSrc } from "@tauri-apps/api/core";

import { api } from "../../api";
import { putIntoComposer } from "../../store";
import { effectiveTheme } from "../../theme";
import { readFrameMessage, wrapPage, type SandboxKind } from "../../widgets/sandboxPage";
import { themedTokensCss } from "../../widgets/tokens";
import { PageKindIcon, PhoneKindIcon, RestartIcon } from "../icons";
import { Phone } from "./Phone";
import { WidgetAction, WidgetCard } from "./WidgetCard";

const SCHEME = "widget";
const FIRST_PAGE_HEIGHT = 160;
const MIN_PAGE_HEIGHT = 48;
const MAX_PAGE_HEIGHT = 900;
const MAX_SENT_TEXT = 4000;

function SandboxFrame(props: { html: string; kind: SandboxKind; title: string; reloadTick: number }) {
  const [address, setAddress] = createSignal<string | null>(null);
  const [problem, setProblem] = createSignal<string | null>(null);
  const [height, setHeight] = createSignal(FIRST_PAGE_HEIGHT);
  let frame: HTMLIFrameElement | undefined;

  createEffect(
    on(
      () => props.html,
      (html) => {
        const page = wrapPage(html, props.kind, themedTokensCss());
        api
          .widgetPage(page)
          .then((id) => setAddress(`${convertFileSrc(id, SCHEME)}?theme=${untrack(effectiveTheme)}`))
          .catch((error) => setProblem(String(error)));
      },
    ),
  );

  const sendTheme = () => frame?.contentWindow?.postMessage({ tarnTheme: effectiveTheme() }, "*");

  createEffect(sendTheme);

  createEffect(
    on(
      () => props.reloadTick,
      () => {
        const current = untrack(address);
        if (frame && current) {
          frame.src = current;
        }
      },
      { defer: true },
    ),
  );

  const receive = (event: MessageEvent) => {
    if (!frame || event.source !== frame.contentWindow) {
      return;
    }
    const message = readFrameMessage(event.data);
    if (message.height !== undefined && props.kind === "page") {
      setHeight(Math.min(MAX_PAGE_HEIGHT, Math.max(MIN_PAGE_HEIGHT, message.height)));
    }
    if (message.send?.trim()) {
      putIntoComposer(message.send.trim().slice(0, MAX_SENT_TEXT));
    }
  };

  window.addEventListener("message", receive);
  onCleanup(() => window.removeEventListener("message", receive));

  return (
    <Show when={address()} fallback={<div class="sandbox-placeholder">{problem() ?? ""}</div>}>
      {(source) => (
        <iframe
          ref={frame}
          class={props.kind === "phone" ? "phone-site" : "page-frame"}
          src={source()}
          sandbox="allow-scripts allow-forms"
          title={props.title}
          style={props.kind === "page" ? { height: `${height()}px` } : undefined}
          onLoad={sendTheme}
        />
      )}
    </Show>
  );
}

export function PageWidget(props: { title: string; html: string }) {
  const [reloadTick, setReloadTick] = createSignal(0);
  return (
    <WidgetCard
      icon={<PageKindIcon />}
      title={props.title}
      flush
      actions={
        <WidgetAction label="Перезапустить" onClick={() => setReloadTick(reloadTick() + 1)}>
          <RestartIcon />
        </WidgetAction>
      }
    >
      <SandboxFrame html={props.html} kind="page" title={props.title} reloadTick={reloadTick()} />
    </WidgetCard>
  );
}

export function PhoneSiteWidget(props: { title: string; html: string }) {
  const [reloadTick, setReloadTick] = createSignal(0);
  return (
    <WidgetCard
      icon={<PhoneKindIcon />}
      title={props.title}
      actions={
        <WidgetAction label="Открыть заново" onClick={() => setReloadTick(reloadTick() + 1)}>
          <RestartIcon />
        </WidgetAction>
      }
    >
      <div class="phone-stage">
        <Phone label={props.title}>
          <div class="phone-site-viewport">
            <SandboxFrame html={props.html} kind="phone" title={props.title} reloadTick={reloadTick()} />
          </div>
        </Phone>
      </div>
    </WidgetCard>
  );
}
