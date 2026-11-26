import { createEffect, on, onCleanup, type Accessor } from "solid-js";

const FOCUSABLE_SELECTOR = 'button:not([disabled]), input:not([disabled]), textarea:not([disabled]), a[href], [tabindex]:not([tabindex="-1"])';
const INITIAL_FOCUS_SELECTOR = '[data-initial-focus="true"]';
const RETURN_FOCUS_DELAY_MS = 160;

function visibleFocusableElements(container: HTMLElement): HTMLElement[] {
  const elements = [...container.querySelectorAll<HTMLElement>(FOCUSABLE_SELECTOR)];
  return elements.filter((element) => element.offsetParent !== null);
}

function keepTabInside(event: KeyboardEvent, container: HTMLElement): void {
  if (event.key !== "Tab") {
    return;
  }
  const elements = visibleFocusableElements(container);
  const first = elements[0];
  const last = elements[elements.length - 1];
  if (!first || !last) {
    return;
  }
  const active = document.activeElement;
  const focusIsOutside = !container.contains(active);
  if (event.shiftKey && (active === first || focusIsOutside)) {
    event.preventDefault();
    last.focus();
  } else if (!event.shiftKey && (active === last || focusIsOutside)) {
    event.preventDefault();
    first.focus();
  }
}

function focusInitialElement(container: HTMLElement | undefined): void {
  if (!container) {
    return;
  }
  const preferred = container.querySelector<HTMLElement>(INITIAL_FOCUS_SELECTOR);
  (preferred ?? visibleFocusableElements(container)[0])?.focus();
}

function returnFocusWhenLost(previous: HTMLElement | null): void {
  window.setTimeout(() => {
    const focusIsLost = document.activeElement === null || document.activeElement === document.body;
    if (!focusIsLost) {
      return;
    }
    if (previous?.isConnected) {
      previous.focus();
    } else {
      document.querySelector<HTMLElement>(".composer-input")?.focus();
    }
  }, RETURN_FOCUS_DELAY_MS);
}

export function createModalFocus(isOpen: Accessor<boolean>, container: () => HTMLElement | undefined): void {
  let previouslyFocused: HTMLElement | null = null;

  const handleKeyDown = (event: KeyboardEvent) => {
    const element = container();
    if (element) {
      keepTabInside(event, element);
    }
  };

  createEffect(on(isOpen, (open, wasOpen) => {
    if (open) {
      previouslyFocused = document.activeElement as HTMLElement | null;
      document.addEventListener("keydown", handleKeyDown);
      window.setTimeout(() => focusInitialElement(container()), 0);
    } else if (wasOpen) {
      document.removeEventListener("keydown", handleKeyDown);
      returnFocusWhenLost(previouslyFocused);
    }
  }));

  onCleanup(() => document.removeEventListener("keydown", handleKeyDown));
}
