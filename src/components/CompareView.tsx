import { For, Show } from "solid-js";

import { formatCost, formatTokens } from "../format";
import { chooseCompareResponse, clearCompareModels, modelDisplayName, state } from "../store";
import { AssistantMessage } from "./MessageView";

export function CompareView() {
  const isFinished = () => state.compareResponses.messages.every((message) => message.status !== "streaming");

  return (
    <div class="compare" style={{ "--columns": state.compareResponses.messages.length }}>
      <For each={state.compareResponses.messages}>
        {(message) => (
          <section class="compare-column">
            <header class="compare-head">
              <span>{modelDisplayName(`${message.provider}/${message.model}`)}</span>
              <small>
                <Show when={message.outputTokens !== null}>{formatTokens(message.outputTokens ?? 0)} ток.</Show>
                <Show when={message.cost !== null}> · {formatCost(message.cost ?? 0)}</Show>
              </small>
            </header>
            <div class="compare-body">
              <AssistantMessage message={message} compact />
            </div>
            <Show when={message.status !== "error"}>
              <footer class="compare-foot">
                <button
                  class="ghost-button"
                  disabled={!isFinished()}
                  onClick={() => {
                    clearCompareModels();
                    void chooseCompareResponse(message);
                  }}
                >
                  Продолжить с этим ответом
                </button>
              </footer>
            </Show>
          </section>
        )}
      </For>
    </div>
  );
}
