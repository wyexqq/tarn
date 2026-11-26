import { Show, createEffect, createSignal, on, onCleanup, onMount } from "solid-js";

import { api } from "../../api";
import { panels } from "../../panels";
import { showToast, state } from "../../store";
import { zoomFactor } from "../../zoom";
import { BackIcon, ForwardIcon, RetryIcon } from "../icons";

const SMALLEST_SHOWN_WIDTH = 40;

function hasPage(url: string): boolean {
  return url !== "" && url !== "about:blank";
}

function addressForField(url: string): string {
  if (!hasPage(url)) {
    return "";
  }
  return url.replace(/^https?:\/\//, "").replace(/\/$/, "");
}

export function BrowserPane() {
  let surface: HTMLDivElement | undefined;
  let addressInput: HTMLInputElement | undefined;
  const [typed, setTyped] = createSignal(addressForField(panels.browser().url));

  const overlayIsOpen = () => state.settingsTab() !== null || state.paletteOpen() || state.viewedImage() !== null;
  const isVisible = () => panels.isOpen() && !overlayIsOpen();

  const sendPlacement = () => {
    if (!surface) {
      return;
    }
    const box = surface.getBoundingClientRect();
    const zoom = zoomFactor();
    const bounds = { x: box.left * zoom, y: box.top * zoom, width: box.width * zoom, height: box.height * zoom };
    void api.browserPlace(bounds, isVisible() && box.width > SMALLEST_SHOWN_WIDTH);
  };

  const go = async (target: string) => {
    try {
      await api.browserGo(target);
      requestAnimationFrame(sendPlacement);
    } catch (error) {
      showToast(String(error), "error");
    }
  };

  const submitAddress = () => {
    const target = typed().trim();
    if (target) {
      void go(target);
      addressInput?.blur();
    }
  };

  createEffect(on(() => panels.browser().url, (url) => {
    if (document.activeElement !== addressInput) {
      setTyped(addressForField(url));
    }
  }));

  createEffect(on([isVisible, zoomFactor], sendPlacement));

  onMount(() => {
    const observer = new ResizeObserver(sendPlacement);
    if (surface) {
      observer.observe(surface);
    }
    window.addEventListener("resize", sendPlacement);
    onCleanup(() => {
      observer.disconnect();
      window.removeEventListener("resize", sendPlacement);
      void api.browserPlace(null, false);
    });
  });

  return (
    <section class="side-pane side-pane-fill">
      <div class="browser-address">
        <button class="icon-button" title="Назад" aria-label="Назад" onClick={() => void go("back")}><BackIcon /></button>
        <button class="icon-button" title="Вперёд" aria-label="Вперёд" onClick={() => void go("forward")}><ForwardIcon /></button>
        <button class="icon-button" title="Обновить" aria-label="Обновить" onClick={() => void go("reload")}><RetryIcon /></button>
        <input
          ref={addressInput}
          value={typed()}
          placeholder="Адрес или поиск"
          aria-label="Адрес"
          spellcheck={false}
          onInput={(event) => setTyped(event.currentTarget.value)}
          onFocus={(event) => event.currentTarget.select()}
          onKeyDown={(event) => event.key === "Enter" && submitAddress()}
        />
      </div>
      <div class="browser-surface" ref={surface}>
        <Show when={!hasPage(panels.browser().url)}>
          <p class="side-empty">Здесь откроются страницы, в том числе localhost</p>
        </Show>
      </div>
      <div class="browser-status" role="status">
        <Show
          when={panels.browser().activity}
          fallback={<span class="browser-title">{panels.browser().isLoading ? "Загружается…" : panels.browser().title}</span>}
        >
          <i class="side-dot is-running" />
          <span class="shimmer">{panels.browser().activity}</span>
        </Show>
      </div>
    </section>
  );
}
