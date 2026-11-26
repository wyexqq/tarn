import { For, Show, createSignal, onCleanup, onMount } from "solid-js";

import { createPresence } from "../motion";
import { CheckIcon, ChevronDownIcon } from "./icons";

export interface ComposerMenuOption {
  value: string;
  title: string;
  hint?: string;
  isRisky?: boolean;
}

export function ComposerMenu(props: {
  id: string;
  title: string;
  buttonText: string;
  value: string;
  options: ComposerMenuOption[];
  side: "left" | "right";
  showNumberKeys?: boolean;
  isDanger?: boolean;
  onChoose: (value: string) => void;
}) {
  let rootElement: HTMLSpanElement | undefined;
  let buttonElement: HTMLButtonElement | undefined;
  const [isOpen, setIsOpen] = createSignal(false);
  const [activeIndex, setActiveIndex] = createSignal(0);
  const presence = createPresence(isOpen);

  const focusActiveOption = () => {
    queueMicrotask(() => rootElement?.querySelector<HTMLElement>(`[data-option="${activeIndex()}"]`)?.focus());
  };

  const open = () => {
    setActiveIndex(Math.max(0, props.options.findIndex((option) => option.value === props.value)));
    setIsOpen(true);
    focusActiveOption();
  };

  const close = () => {
    setIsOpen(false);
    buttonElement?.focus();
  };

  const choose = (option: ComposerMenuOption) => {
    close();
    props.onChoose(option.value);
  };

  const moveActive = (offset: number) => {
    const count = props.options.length;
    setActiveIndex((activeIndex() + offset + count) % count);
    focusActiveOption();
  };

  const handleKeyDown = (event: KeyboardEvent) => {
    if (!isOpen()) {
      return;
    }
    const numberKeyOption = props.showNumberKeys ? props.options[Number(event.key) - 1] : undefined;
    if (event.key === "Escape") {
      event.preventDefault();
      close();
    } else if (event.key === "ArrowDown") {
      event.preventDefault();
      moveActive(1);
    } else if (event.key === "ArrowUp") {
      event.preventDefault();
      moveActive(-1);
    } else if (numberKeyOption) {
      event.preventDefault();
      choose(numberKeyOption);
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
    <span class="menu-anchor" ref={rootElement} onKeyDown={handleKeyDown}>
      <button
        id={props.id}
        ref={buttonElement}
        class="menu-button"
        classList={{ "is-danger": props.isDanger }}
        aria-haspopup="menu"
        aria-expanded={isOpen()}
        onClick={() => (isOpen() ? close() : open())}
      >
        <span class="menu-button-label">{props.buttonText}</span>
        <ChevronDownIcon />
      </button>
      <Show when={presence.mounted()}>
        <div class={`composer-menu menu-from-${props.side}`} classList={{ "is-closing": presence.closing() }} role="menu" aria-label={props.title}>
          <div class="composer-menu-title">{props.title}</div>
          <For each={props.options}>
            {(option, index) => (
              <button
                class="composer-menu-option"
                classList={{ "is-risky": option.isRisky, "is-active": activeIndex() === index() }}
                role="menuitemradio"
                aria-checked={option.value === props.value}
                data-option={index()}
                tabindex={activeIndex() === index() ? 0 : -1}
                onMouseMove={() => setActiveIndex(index())}
                onClick={() => choose(option)}
              >
                <span class="composer-menu-text">
                  <strong>{option.title}</strong>
                  <Show when={option.hint}><small>{option.hint}</small></Show>
                </span>
                <span class="composer-menu-check">
                  <Show when={option.value === props.value}><CheckIcon /></Show>
                </span>
                <span class="composer-menu-key">{props.showNumberKeys ? index() + 1 : ""}</span>
              </button>
            )}
          </For>
        </div>
      </Show>
    </span>
  );
}
