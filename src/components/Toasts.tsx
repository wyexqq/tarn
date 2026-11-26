import { For, Show } from "solid-js";

import { dismissToast, state } from "../store";

export function Toasts() {
  return (
    <div class="toasts" role="status" aria-live="polite">
      <For each={state.toasts()}>
        {(toast) => (
          <div class="toast" classList={{ "toast-error": toast.tone === "error" }}>
            {toast.text}
            <Show when={toast.action}>
              {(action) => (
                <button
                  class="toast-action"
                  onClick={() => {
                    dismissToast(toast.id);
                    action().run();
                  }}
                >
                  {action().text}
                </button>
              )}
            </Show>
          </div>
        )}
      </For>
    </div>
  );
}
