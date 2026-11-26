import { createEffect, createSignal, onCleanup, type Accessor } from "solid-js";

const EXIT_DURATION_MS = 130;

function prefersReducedMotion(): boolean {
  return window.matchMedia("(prefers-reduced-motion: reduce)").matches;
}

export function withViewTransition(update: () => void): void {
  const canAnimate = "startViewTransition" in document && !prefersReducedMotion() && document.visibilityState === "visible";
  if (canAnimate) {
    document.startViewTransition(update);
  } else {
    update();
  }
}

export interface Presence {
  mounted: Accessor<boolean>;
  closing: Accessor<boolean>;
}

export function createPresence(visible: Accessor<boolean>): Presence {
  const [mounted, setMounted] = createSignal(visible());
  const [closing, setClosing] = createSignal(false);
  let timer = 0;

  createEffect(() => {
    window.clearTimeout(timer);
    if (visible()) {
      setClosing(false);
      setMounted(true);
      return;
    }
    if (!mounted()) {
      return;
    }
    setClosing(true);
    timer = window.setTimeout(() => {
      setMounted(false);
      setClosing(false);
    }, prefersReducedMotion() ? 0 : EXIT_DURATION_MS);
  });

  onCleanup(() => window.clearTimeout(timer));
  return { mounted, closing };
}
