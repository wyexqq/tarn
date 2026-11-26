import { For, Show, createEffect, createResource, createSignal, on } from "solid-js";

import { api, type MemoryFile } from "../api";
import { formatTokens } from "../format";
import { modelRefOf, providerName, showToast, state, updateSetting } from "../store";
import { Select } from "./Select";

const SOURCE_TOGGLES = [
  { key: "memoryTarn", id: "memory-tarn", label: "Своя память Tarn" },
  { key: "memoryClaude", id: "memory-claude", label: "CLAUDE.md и память Claude Code" },
  { key: "memoryCodex", id: "memory-codex", label: "AGENTS.md из Codex" },
];

const SOURCE_NAMES: Record<MemoryFile["source"], string> = { tarn: "Tarn", claude: "Claude Code", codex: "Codex" };

const DEFAULT_COMPACT_TOKENS = "400000";
const CHARS_PER_TOKEN_ESTIMATE = 3;

function SavedTextArea(props: { id: string; label: string; initial: string; onSave: (text: string) => Promise<void> }) {
  const [text, setText] = createSignal(props.initial);
  createEffect(on(() => props.initial, (initial) => setText(initial), { defer: true }));

  const save = async () => {
    if (text() === props.initial) {
      return;
    }
    try {
      await props.onSave(text());
    } catch (error) {
      showToast(String(error), "error");
    }
  };

  return (
    <label class="field-stack">
      <span>{props.label}</span>
      <textarea
        id={props.id}
        class="text-input prompt-input"
        value={text()}
        onInput={(event) => setText(event.currentTarget.value)}
        onBlur={() => void save()}
      />
    </label>
  );
}

export function MemoryTab() {
  const chatId = () => state.currentChatId();
  const [own, { refetch: refetchOwn }] = createResource(chatId, (id) => api.ownMemory(id), { initialValue: { shared: "", projectText: null, projectFolder: null } });
  const [files, { refetch: refetchFiles }] = createResource(chatId, (id) => api.memoryFiles(id), { initialValue: [] });
  const [compactText, setCompactText] = createSignal(state.settings().autoCompactTokens ?? DEFAULT_COMPACT_TOKENS);

  const saveOwn = async (place: "shared" | "project", text: string) => {
    await api.saveOwnMemory(place, chatId(), text);
    await refetchOwn();
    await refetchFiles();
  };

  const toggleSource = async (key: string, isOn: boolean) => {
    await updateSetting(key, String(isOn));
    await refetchFiles();
  };

  const saveCompact = () => {
    const digitsOnly = compactText().replace(/\D/g, "");
    setCompactText(digitsOnly);
    void updateSetting("autoCompactTokens", digitsOnly || "0");
  };

  return (
    <div class="settings-page">
      <h2>Память</h2>

      <For each={SOURCE_TOGGLES}>
        {(toggle) => (
          <label class="check-row">
            <span>{toggle.label}</span>
            <input
              id={toggle.id}
              class="toggle"
              type="checkbox"
              checked={state.settings()[toggle.key] !== "false"}
              onChange={(event) => void toggleSource(toggle.key, event.currentTarget.checked)}
            />
          </label>
        )}
      </For>

      <label class="field-row">
        <span>Сжимать контекст от, токенов (0 выключает)</span>
        <input
          id="auto-compact-tokens"
          class="text-input short-input"
          inputmode="numeric"
          value={compactText()}
          onInput={(event) => setCompactText(event.currentTarget.value)}
          onBlur={saveCompact}
        />
      </label>

      <div class="field-row">
        <span>Кто пишет конспект при сжатии</span>
        <Select
          id="compact-model-select"
          label="Модель для сжатия"
          value={state.settings().compactModel ?? ""}
          options={[
            { value: "", label: "Самая дешёвая модель провайдера" },
            ...state.models().map((model) => ({ value: modelRefOf(model), label: `${providerName(model.provider)}: ${model.name}` })),
          ]}
          onChange={(value) => void updateSetting("compactModel", value)}
        />
      </div>

      <SavedTextArea id="memory-shared" label="Общая память" initial={own().shared} onSave={(text) => saveOwn("shared", text)} />

      <Show when={own().projectFolder}>
        {(folder) => (
          <SavedTextArea
            id="memory-project"
            label={`Память проекта ${folder()}`}
            initial={own().projectText ?? ""}
            onSave={(text) => saveOwn("project", text)}
          />
        )}
      </Show>

      <Show when={files().length > 0}>
        <section>
          <h3>Модель сейчас читает</h3>
          <div class="table-scroll">
            <table class="data-table">
              <thead><tr><th>Откуда</th><th>Файл</th><th>Токены</th></tr></thead>
              <tbody>
                <For each={files()}>
                  {(file) => (
                    <tr>
                      <td>{SOURCE_NAMES[file.source]}</td>
                      <td><code title={file.path}>{file.path}</code><Show when={file.wasCut}> (обрезан)</Show></td>
                      <td>{formatTokens(Math.round(file.text.length / CHARS_PER_TOKEN_ESTIMATE))}</td>
                    </tr>
                  )}
                </For>
              </tbody>
            </table>
          </div>
        </section>
      </Show>
    </div>
  );
}
