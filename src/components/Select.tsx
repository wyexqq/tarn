import { For, Show, createEffect, createMemo, createSignal, onCleanup, onMount } from "solid-js";
import { Portal } from "solid-js/web";

import { createPresence } from "../motion";
import { CheckIcon, ChevronDownIcon } from "./icons";

export interface SelectOption {
  value: string;
  label: string;
}

const MENU_MAX_HEIGHT_PX = 280;
const MENU_GAP_PX = 4;
const WINDOW_MARGIN_PX = 8;

export function Select(props: {
  id: string;
  label: string;
  value: string;
  options: SelectOption[];
  onChange: (value: string) => void;
}) {
  let buttonElement: HTMLButtonElement | undefined;
  let menuElement: HTMLDivElement | undefined;
  const [isOpen, setIsOpen] = createSignal(false);
  const [activeIndex, setActiveIndex] = createSignal(0);
  const [menuStyle, setMenuStyle] = createSignal<Record<string, string>>({});
  const presence = createPresence(isOpen);

  const selectedLabel = createMemo(
    () => props.options.find((option) => option.value === props.value)?.label ?? props.options[0]?.label ?? "",
  );

  const placeMenu = () => {
    if (!buttonElement) {
      return;
    }
    const box = buttonElement.getBoundingClientRect();
    const spaceBelow = window.innerHeight - box.bottom - WINDOW_MARGIN_PX;
    const opensUp = spaceBelow < Math.min(MENU_MAX_HEIGHT_PX, 160) && box.top > spaceBelow;
    const height = Math.min(MENU_MAX_HEIGHT_PX, (opensUp ? box.top : spaceBelow) - MENU_GAP_PX);
    const width = Math.max(box.width, 200);
    const left = Math.min(box.left, window.innerWidth - width - WINDOW_MARGIN_PX);
    setMenuStyle({
      left: `${left}px`,
      width: `${width}px`,
      "max-height": `${height}px`,
      ...(opensUp
        ? { bottom: `${window.innerHeight - box.top + MENU_GAP_PX}px`, "transform-origin": "bottom" }
        : { top: `${box.bottom + MENU_GAP_PX}px`, "transform-origin": "top" }),
    });
  };

  const open = () => {
    placeMenu();
    setActiveIndex(Math.max(0, props.options.findIndex((option) => option.value === props.value)));
    setIsOpen(true);
  };

  const close = () => {
    setIsOpen(false);
    buttonElement?.focus();
  };

  const choose = (option: SelectOption) => {
    props.onChange(option.value);
    close();
  };

  const moveActive = (offset: number) => {
    const lastIndex = props.options.length - 1;
    setActiveIndex((index) => Math.min(lastIndex, Math.max(0, index + offset)));
  };

  const handleKeyDown = (event: KeyboardEvent) => {
    if (!isOpen()) {
      if (event.key === "ArrowDown" || event.key === "ArrowUp") {
        event.preventDefault();
        open();
      }
      return;
    }
    if (event.key === "ArrowDown") {
      event.preventDefault();
      moveActive(1);
    } else if (event.key === "ArrowUp") {
      event.preventDefault();
      moveActive(-1);
    } else if (event.key === "Enter" || event.key === " ") {
      event.preventDefault();
      choose(props.options[activeIndex()]);
    } else if (event.key === "Escape") {
      event.preventDefault();
      close();
    } else if (event.key === "Tab") {
      setIsOpen(false);
    }
  };

  const closeOnOutsidePress = (event: MouseEvent) => {
    const target = event.target as Node;
    if (isOpen() && !buttonElement?.contains(target) && !menuElement?.contains(target)) {
      setIsOpen(false);
    }
  };

  createEffect(() => {
    const index = activeIndex();
    if (isOpen()) {
      menuElement?.querySelector(`[data-index="${index}"]`)?.scrollIntoView({ block: "nearest" });
    }
  });

  onMount(() => document.addEventListener("mousedown", closeOnOutsidePress));
  onCleanup(() => document.removeEventListener("mousedown", closeOnOutsidePress));

  return (
    <>
      <button
        ref={buttonElement}
        id={props.id}
        type="button"
        class="select-button"
        role="combobox"
        aria-label={props.label}
        aria-haspopup="listbox"
        aria-expanded={isOpen()}
        onClick={() => (isOpen() ? close() : open())}
        onKeyDown={handleKeyDown}
      >
        <span class="select-value">{selectedLabel()}</span>
        <ChevronDownIcon />
      </button>
      <Show when={presence.mounted()}>
        <Portal>
          <div
            ref={menuElement}
            class="select-menu"
            classList={{ "is-closing": presence.closing() }}
            role="listbox"
            aria-label={props.label}
            style={menuStyle()}
          >
            <For each={props.options}>
              {(option, index) => (
                <button
                  type="button"
                  class="select-option"
                  classList={{ "is-active": activeIndex() === index() }}
                  data-index={index()}
                  role="option"
                  aria-selected={option.value === props.value}
                  onMouseMove={() => setActiveIndex(index())}
                  onClick={() => choose(option)}
                >
                  <span class="select-option-label">{option.label}</span>
                  <Show when={option.value === props.value}>
                    <span class="model-row-check"><CheckIcon /></span>
                  </Show>
                </button>
              )}
            </For>
          </div>
        </Portal>
      </Show>
    </>
  );
}
