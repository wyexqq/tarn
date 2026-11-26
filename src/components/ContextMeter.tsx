import { For, Show, createMemo, createSignal, onCleanup, onMount } from "solid-js";

import type { ContextPart } from "../api";
import { formatThousands } from "../format";
import { createPresence } from "../motion";
import { compactNow, panels, refreshContextUsage } from "../panels";
import { modelDisplayName } from "../store";

const RING_RADIUS = 6;
const RING_LENGTH = 2 * Math.PI * RING_RADIUS;
const NEARLY_FULL_SHARE = 0.85;

const KIND_COLORS: Record<ContextPart["kind"], string> = {
  messages: "var(--seg-1)",
  summary: "var(--seg-2)",
  tools: "var(--seg-3)",
  images: "var(--seg-4)",
  memory: "var(--seg-5)",
  instruction: "var(--seg-6)",
};

function percentOf(share: number): string {
  return `${Math.round(share * 100)}%`;
}

function PartRow(props: { part: ContextPart }) {
  return (
    <div class="context-part">
      <i style={{ background: KIND_COLORS[props.part.kind] }} />
      <span>{props.part.title}</span>
      <span class="context-part-tokens">{formatThousands(props.part.tokens)}</span>
      <Show when={props.part.path}>
        <code title={props.part.path ?? ""}>{props.part.path}</code>
      </Show>
    </div>
  );
}

function ContextDetails(props: { share: number }) {
  const usage = panels.contextUsage;
  const limit = createMemo(() => usage()?.compactAtTokens ?? usage()?.modelLimitTokens ?? null);
  const shownParts = createMemo(() => usage()?.parts.filter((part) => part.tokens > 0) ?? []);

  const widthOf = (tokens: number) => {
    const whole = Math.max(limit() ?? 0, usage()?.usedTokens ?? 0, 1);
    return `${Math.min(100, (tokens / whole) * 100)}%`;
  };

  const compactionText = () => {
    const compactAt = usage()?.compactAtTokens;
    return compactAt ? `Сожмётся сам на ${formatThousands(compactAt)}` : "Автосжатие выключено";
  };

  const summarizerText = () => {
    const modelRef = usage()?.compactModelRef;
    return modelRef ? `Конспект пишет ${modelDisplayName(modelRef)}` : "";
  };

  return (
    <>
      <div class="context-head">
        <p class="context-total">
          {formatThousands(usage()?.usedTokens ?? 0)}
          <Show when={limit()}>{(shownLimit) => <span> из {formatThousands(shownLimit())}</span>}</Show>
        </p>
        <span class="context-share">{percentOf(props.share)}</span>
      </div>
      <div class="context-bar" aria-hidden="true">
        <For each={shownParts()}>{(part) => <i style={{ width: widthOf(part.tokens), background: KIND_COLORS[part.kind] }} />}</For>
      </div>
      <div class="context-parts">
        <For each={shownParts()}>{(part) => <PartRow part={part} />}</For>
      </div>
      <div class="context-foot">
        <span class="context-foot-text">
          <small>{compactionText()}</small>
          <Show when={summarizerText()}><small>{summarizerText()}</small></Show>
        </span>
        <button class="ghost-button" disabled={panels.isCompacting()} onClick={() => void compactNow()}>
          {panels.isCompacting() ? "Сжимаю…" : "Сжать сейчас"}
        </button>
      </div>
    </>
  );
}

export function ContextMeter() {
  let rootElement: HTMLSpanElement | undefined;
  let buttonElement: HTMLButtonElement | undefined;
  const [isOpen, setIsOpen] = createSignal(false);
  const presence = createPresence(isOpen);

  const share = createMemo(() => {
    const usage = panels.contextUsage();
    const limit = usage?.compactAtTokens ?? usage?.modelLimitTokens ?? null;
    if (!usage || !limit || usage.usedTokens === 0) {
      return null;
    }
    return Math.min(1, usage.usedTokens / limit);
  });

  const title = () => {
    const usage = panels.contextUsage();
    return usage ? `Контекст: ${formatThousands(usage.usedTokens)} токенов` : "Контекст";
  };

  const open = () => {
    setIsOpen(true);
    void refreshContextUsage();
  };

  const close = () => {
    setIsOpen(false);
    buttonElement?.focus();
  };

  const closeOnEscape = (event: KeyboardEvent) => {
    if (isOpen() && event.key === "Escape") {
      event.preventDefault();
      close();
    }
  };

  const closeOnOutsideClick = (event: MouseEvent) => {
    if (isOpen() && rootElement && !rootElement.contains(event.target as Node)) {
      setIsOpen(false);
    }
  };

  onMount(() => document.addEventListener("mousedown", closeOnOutsideClick));
  onCleanup(() => document.removeEventListener("mousedown", closeOnOutsideClick));

  return (
    <Show when={share() !== null}>
      <span class="menu-anchor" ref={rootElement} onKeyDown={closeOnEscape}>
        <button
          ref={buttonElement}
          class="context-meter"
          classList={{ "is-full": (share() ?? 0) > NEARLY_FULL_SHARE }}
          title={title()}
          aria-label={title()}
          aria-haspopup="dialog"
          aria-expanded={isOpen()}
          onClick={() => (isOpen() ? close() : open())}
        >
          <svg width="16" height="16" viewBox="0 0 16 16" fill="none" stroke-width="2" aria-hidden="true">
            <circle cx="8" cy="8" r={RING_RADIUS} class="context-meter-track" />
            <circle
              cx="8"
              cy="8"
              r={RING_RADIUS}
              class="context-meter-fill"
              stroke-dasharray={`${(share() ?? 0) * RING_LENGTH} ${RING_LENGTH}`}
              transform="rotate(-90 8 8)"
              stroke-linecap="round"
            />
          </svg>
          <span>{percentOf(share() ?? 0)}</span>
        </button>
        <Show when={presence.mounted()}>
          <div class="context-popover" classList={{ "is-closing": presence.closing() }} role="dialog" aria-label="Контекст">
            <ContextDetails share={share() ?? 0} />
          </div>
        </Show>
      </span>
    </Show>
  );
}
