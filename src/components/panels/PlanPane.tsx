import { For, Show, createMemo } from "solid-js";

import { panels } from "../../panels";
import { chooseAccessMode, currentAccess, focusComposer, isStreaming, sendMessage } from "../../store";
import { CheckIcon } from "../icons";

export function PlanPane() {
  const unfinished = createMemo(() => panels.plan().filter((step) => step.status !== "done").length);

  const carryOut = async () => {
    if (currentAccess().mode === "plan") {
      await chooseAccessMode("acceptEdits");
    }
    await sendMessage("Выполняй план", []);
  };

  return (
    <section class="side-pane">
      <Show when={panels.plan().length > 0} fallback={<p class="side-empty">Плана пока нет</p>}>
        <div class="side-section">
          <For each={panels.plan()}>
            {(step) => (
              <div class="plan-step" classList={{ "is-done": step.status === "done", "is-now": step.status === "in_progress" }}>
                <span class="plan-check">
                  <Show when={step.status === "done"}><CheckIcon /></Show>
                </span>
                <span>{step.text}</span>
              </div>
            )}
          </For>
        </div>
        <Show when={unfinished() > 0 && !isStreaming()}>
          <div class="side-section plan-actions">
            <button class="primary-button" onClick={() => void carryOut()}>Выполнять</button>
            <button class="ghost-button" onClick={focusComposer}>Изменить</button>
          </div>
        </Show>
      </Show>
    </section>
  );
}
