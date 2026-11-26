import { For, Match, Show, Switch, createEffect, createMemo, createResource, createSignal, on } from "solid-js";
import { open } from "@tauri-apps/plugin-dialog";

import { api, type ImportSource, type PriceEntry, type ProviderInfo } from "../api";
import { createModalFocus } from "../focus";
import { formatBytes, formatClock, formatCost, formatRelativeDay, formatTokens, pluralRu } from "../format";
import { createPresence } from "../motion";
import {
  actions,
  applyTheme,
  modelRefOf,
  providerName,
  refreshProviders,
  showToast,
  state,
  updateSetting,
  type SettingsTab,
} from "../store";
import { CloseIcon, ImportIcon, TrashIcon } from "./icons";
import { Select } from "./Select";
import { MemoryTab } from "./SettingsMemory";
import { AccessTab, PluginsTab, SkillsTab, VoiceTab } from "./SettingsExtensions";

const TABS: { id: SettingsTab; label: string }[] = [
  { id: "keys", label: "Ключи" },
  { id: "access", label: "Доступ" },
  { id: "skills", label: "Скиллы" },
  { id: "plugins", label: "Плагины" },
  { id: "voice", label: "Голос" },
  { id: "memory", label: "Память" },
  { id: "import", label: "Импорт" },
  { id: "usage", label: "Расходы" },
  { id: "log", label: "Журнал" },
  { id: "general", label: "Общие" },
];

export function Settings() {
  const isOpen = () => state.settingsTab() !== null;
  const presence = createPresence(isOpen);
  const [lastTab, setLastTab] = createSignal<SettingsTab>("keys");
  let dialogElement: HTMLDivElement | undefined;
  createModalFocus(isOpen, () => dialogElement);

  const moveBetweenTabs = (event: KeyboardEvent) => {
    const offset = event.key === "ArrowDown" ? 1 : event.key === "ArrowUp" ? -1 : 0;
    if (offset === 0) {
      return;
    }
    event.preventDefault();
    const currentIndex = TABS.findIndex((tab) => tab.id === lastTab());
    const nextTab = TABS[(currentIndex + offset + TABS.length) % TABS.length];
    actions.setSettingsTab(nextTab.id);
    queueMicrotask(() => dialogElement?.querySelector<HTMLElement>('.settings-tab[data-initial-focus="true"]')?.focus());
  };

  createEffect(() => {
    const tab = state.settingsTab();
    if (tab) {
      setLastTab(tab);
    }
  });

  return (
    <Show when={presence.mounted()}>
      <div
        class="overlay settings-overlay"
        classList={{ "is-closing": presence.closing() }}
        onMouseDown={(event) => {
          if (event.target === event.currentTarget) {
            actions.setSettingsTab(null);
          }
        }}
      >
        <div class="settings" ref={dialogElement} role="dialog" aria-modal="true" aria-label="Настройки" classList={{ "is-closing": presence.closing() }}>
          <nav class="settings-nav" role="tablist" aria-orientation="vertical" onKeyDown={moveBetweenTabs}>
            <For each={TABS}>
              {(tab) => (
                <button
                  class="settings-tab"
                  role="tab"
                  data-initial-focus={lastTab() === tab.id ? "true" : "false"}
                  tabindex={lastTab() === tab.id ? 0 : -1}
                  classList={{ "is-active": lastTab() === tab.id }}
                  aria-selected={lastTab() === tab.id}
                  onClick={() => actions.setSettingsTab(tab.id)}
                >
                  {tab.label}
                </button>
              )}
            </For>
          </nav>
          <div class="settings-content">
            <button class="icon-button settings-close" aria-label="Закрыть настройки" onClick={() => actions.setSettingsTab(null)}>
              <CloseIcon />
            </button>
            <Switch>
              <Match when={lastTab() === "keys"}><KeysTab /></Match>
              <Match when={lastTab() === "access"}><AccessTab /></Match>
              <Match when={lastTab() === "skills"}><SkillsTab /></Match>
              <Match when={lastTab() === "plugins"}><PluginsTab /></Match>
              <Match when={lastTab() === "voice"}><VoiceTab /></Match>
              <Match when={lastTab() === "memory"}><MemoryTab /></Match>
              <Match when={lastTab() === "import"}><ImportTab /></Match>
              <Match when={lastTab() === "usage"}><UsageTab /></Match>
              <Match when={lastTab() === "log"}><LogTab /></Match>
              <Match when={lastTab() === "general"}><GeneralTab /></Match>
            </Switch>
          </div>
        </div>
      </div>
    </Show>
  );
}

function providerStatusText(provider: ProviderInfo): string {
  if (provider.modelCount > 0) {
    return `${provider.modelCount} ${pluralRu(provider.modelCount, "модель", "модели", "моделей")}`;
  }
  if (provider.isLocal) {
    return "не запущен";
  }
  return provider.hasKey ? "ключ сохранён" : "нет ключа";
}

function KeysTab() {
  const [keyText, setKeyText] = createSignal("");
  const [manualProvider, setManualProvider] = createSignal("");
  const [isChecking, setIsChecking] = createSignal(false);
  const [keyError, setKeyError] = createSignal<string | null>(null);
  const [showCustom, setShowCustom] = createSignal(false);

  const addKey = async (event: Event) => {
    event.preventDefault();
    setIsChecking(true);
    setKeyError(null);
    try {
      const result = await api.addKey(keyText(), manualProvider() || null);
      await refreshProviders();
      setKeyText("");
      setManualProvider("");
      showToast(`${result.providerName} подключён: ${result.modelCount} ${pluralRu(result.modelCount, "модель", "модели", "моделей")}`);
    } catch (error) {
      setKeyError(String(error));
    } finally {
      setIsChecking(false);
    }
  };

  const removeProvider = async (provider: ProviderInfo) => {
    if (provider.isCustom) {
      await api.deleteCustomProvider(provider.id);
    } else {
      await api.deleteKey(provider.id);
    }
    await refreshProviders();
    showToast(`${provider.name} отключён`);
  };

  const keyedProviders = createMemo(() => state.providers().filter((provider) => provider.requiresKey));

  return (
    <div class="settings-page">
      <h2>Ключи</h2>
      <form class="key-form" onSubmit={(event) => void addKey(event)}>
        <input
          id="key-input"
          class="text-input key-input"
          type="password"
          autocomplete="off"
          spellcheck={false}
          placeholder="sk-…, AIza…, gsk_…"
          aria-label="API-ключ"
          value={keyText()}
          onInput={(event) => setKeyText(event.currentTarget.value)}
        />
        <Select
          id="key-provider"
          label="Провайдер"
          value={manualProvider()}
          options={[{ value: "", label: "Определить сам" }, ...keyedProviders().map((provider) => ({ value: provider.id, label: provider.name }))]}
          onChange={setManualProvider}
        />
        <button class="primary-button" type="submit" disabled={isChecking() || keyText().trim().length === 0}>
          {isChecking() ? "Проверяю…" : "Добавить"}
        </button>
      </form>
      <Show when={keyError()}>
        <p class="form-error" role="alert">{keyError()}</p>
      </Show>

      <div class="provider-list">
        <For each={state.providers()}>
          {(provider) => (
            <div class="provider-row">
              <i class="status-dot" classList={{ "is-off": !provider.connected }} />
              <span class="provider-name">{provider.name}</span>
              <Show when={provider.isLocal}><span class="tag">на этом ПК</span></Show>
              <span class="provider-status">{providerStatusText(provider)}</span>
              <Show when={provider.hasKey || provider.isCustom}>
                <button class="action-button" title="Удалить ключ" aria-label={`Удалить ${provider.name}`} onClick={() => void removeProvider(provider)}>
                  <TrashIcon />
                </button>
              </Show>
            </div>
          )}
        </For>
      </div>

      <Show when={showCustom()} fallback={<button class="link-button" onClick={() => setShowCustom(true)}>Подключить свой адрес API</button>}>
        <CustomProviderForm onDone={() => setShowCustom(false)} />
      </Show>
    </div>
  );
}

function CustomProviderForm(props: { onDone: () => void }) {
  const [name, setName] = createSignal("");
  const [baseUrl, setBaseUrl] = createSignal("");
  const [protocol, setProtocol] = createSignal("openai");
  const [key, setKey] = createSignal("");
  const [error, setError] = createSignal<string | null>(null);
  const [isChecking, setIsChecking] = createSignal(false);

  const submit = async (event: Event) => {
    event.preventDefault();
    setIsChecking(true);
    setError(null);
    try {
      const result = await api.addCustomProvider({ name: name(), baseUrl: baseUrl(), protocol: protocol(), key: key() || null });
      await refreshProviders();
      showToast(`${result.providerName} подключён`);
      props.onDone();
    } catch (failure) {
      setError(String(failure));
    } finally {
      setIsChecking(false);
    }
  };

  return (
    <form class="custom-form" onSubmit={(event) => void submit(event)}>
      <input id="custom-name" class="text-input" placeholder="Название" aria-label="Название" value={name()} onInput={(event) => setName(event.currentTarget.value)} required />
      <input id="custom-url" class="text-input" placeholder="http://localhost:8000/v1" aria-label="Адрес API" value={baseUrl()} onInput={(event) => setBaseUrl(event.currentTarget.value)} required />
      <Select
        id="custom-protocol"
        label="Формат API"
        value={protocol()}
        options={[
          { value: "openai", label: "Формат OpenAI" },
          { value: "anthropic", label: "Формат Anthropic" },
        ]}
        onChange={setProtocol}
      />
      <input id="custom-key" class="text-input" type="password" placeholder="Ключ, если нужен" aria-label="Ключ" value={key()} onInput={(event) => setKey(event.currentTarget.value)} />
      <Show when={error()}><p class="form-error" role="alert">{error()}</p></Show>
      <div class="custom-actions">
        <button class="ghost-button" type="button" onClick={props.onDone}>Отмена</button>
        <button class="primary-button" type="submit" disabled={isChecking()}>{isChecking() ? "Проверяю…" : "Подключить"}</button>
      </div>
    </form>
  );
}

const SOURCE_TITLES = { claude_code: "Claude Code", codex: "Codex" } as const;

function ImportTab() {
  const [sources, { refetch }] = createResource(() => api.scanImportSources());
  const [selected, setSelected] = createSignal<Set<string>>(new Set());
  const [customPaths, setCustomPaths] = createSignal<Record<string, string>>({});

  const progress = state.importProgress;
  const isRunning = () => {
    const current = progress();
    return current !== null && !current.finished;
  };

  createEffect(on(sources, (list) => {
    if (list && selected().size === 0) {
      setSelected(new Set(list.filter((source) => source.exists && source.fileCount > 0).map((source) => source.source)));
    }
  }));

  createEffect(on(() => progress()?.finished, (finished) => {
    if (finished) {
      void refetch();
    }
  }, { defer: true }));

  const toggle = (source: string) => {
    setSelected((current) => {
      const updated = new Set(current);
      if (updated.has(source)) {
        updated.delete(source);
      } else {
        updated.add(source);
      }
      return updated;
    });
  };

  const pickFolder = async (source: ImportSource) => {
    const folder = await open({ directory: true, title: `Папка с чатами ${SOURCE_TITLES[source.source]}` });
    if (typeof folder === "string") {
      setCustomPaths((current) => ({ ...current, [source.source]: folder }));
      await api.setSetting(`importPath.${source.source}`, folder);
      await refetch();
    }
  };

  const runImport = async () => {
    const list = (sources() ?? [])
      .filter((source) => selected().has(source.source))
      .map((source) => ({ source: source.source, path: customPaths()[source.source] ?? source.path }));
    if (list.length === 0) {
      return;
    }
    try {
      await api.startImport(list);
    } catch (error) {
      showToast(String(error), "error");
    }
  };

  const progressShare = () => {
    const current = progress();
    return current && current.filesTotal > 0 ? current.filesDone / current.filesTotal : 0;
  };

  return (
    <div class="settings-page">
      <h2>Импорт чатов</h2>

      <div class="import-sources">
        <For each={sources() ?? []}>
          {(source) => (
            <div class="import-source">
              <input
                id={`import-${source.source}`}
                type="checkbox"
                checked={selected().has(source.source)}
                disabled={!source.exists || source.fileCount === 0}
                onChange={() => toggle(source.source)}
              />
              <label for={`import-${source.source}`} class="import-source-main">
                <span class="import-source-title">{SOURCE_TITLES[source.source]}</span>
                <code class="import-path">{source.path}</code>
                <span class="import-source-meta">
                  <Show when={source.exists} fallback="Папка не найдена">
                    {source.fileCount} {pluralRu(source.fileCount, "файл", "файла", "файлов")} · {formatBytes(source.totalBytes)}
                    <Show when={source.importedChats > 0}> · уже импортировано: {source.importedChats}</Show>
                  </Show>
                </span>
              </label>
              <button class="link-button" onClick={() => void pickFolder(source)}>Другая папка</button>
            </div>
          )}
        </For>
      </div>

      <label class="check-row">
        <span>Подтягивать новые чаты автоматически</span>
        <input
          id="import-watch"
          class="toggle"
          type="checkbox"
          checked={state.settings().importWatch === "true"}
          onChange={(event) => void updateSetting("importWatch", String(event.currentTarget.checked))}
        />
      </label>

      <Show when={progress()}>
        {(current) => (
          <div class="progress-block" role="status">
            <div class="progress-track"><div class="progress-fill" style={{ transform: `scaleX(${progressShare()})` }} /></div>
            <span class="progress-text">
              <Show when={current().finished} fallback={`Читаю файлы: ${current().filesDone} из ${current().filesTotal}`}>
                Готово. Новых и обновлённых чатов: {current().chatsImported}
                <Show when={current().failedFiles > 0}>. Не прочитано файлов: {current().failedFiles}</Show>
              </Show>
            </span>
          </div>
        )}
      </Show>

      <div class="settings-actions">
        <button class="primary-button" disabled={isRunning() || selected().size === 0} onClick={() => void runImport()}>
          <ImportIcon />
          {isRunning() ? "Импортирую…" : "Импортировать"}
        </button>
      </div>
    </div>
  );
}

function UsageTab() {
  const [usage, { refetch }] = createResource(() => api.usageSummary());
  const [prices, { refetch: refetchPrices }] = createResource(() => api.listPrices());
  const [budgetText, setBudgetText] = createSignal(state.settings().monthlyBudget ?? "");

  createEffect(on(state.usageTick, () => void refetch(), { defer: true }));

  const budgetShare = () => {
    const summary = usage();
    return summary?.budget ? Math.min(summary.monthCost / summary.budget, 1) : 0;
  };

  const saveBudget = async () => {
    await updateSetting("monthlyBudget", budgetText().trim().replace(",", "."));
    await refetch();
  };

  return (
    <div class="settings-page">
      <h2>Расходы</h2>

      <Show when={usage()}>
        {(summary) => (
          <>
            <div class="usage-grid">
              <div class="usage-cell"><span>Сегодня</span><strong>{formatCost(summary().todayCost)}</strong></div>
              <div class="usage-cell"><span>Этот месяц</span><strong>{formatCost(summary().monthCost)}</strong></div>
              <div class="usage-cell"><span>Токены за месяц</span><strong>{formatTokens(summary().monthInputTokens + summary().monthOutputTokens)}</strong></div>
            </div>
            <Show when={summary().budget}>
              {(budget) => (
                <div class="progress-block">
                  <div class="progress-track">
                    <div class="progress-fill" classList={{ "is-warning": budgetShare() >= 0.8 }} style={{ transform: `scaleX(${budgetShare()})` }} />
                  </div>
                  <span class="progress-text">{formatCost(summary().monthCost)} из {formatCost(budget())} за месяц</span>
                </div>
              )}
            </Show>
            <Show when={summary().unpricedMessages > 0}>
              <p class="settings-note">Без цены: {summary().unpricedMessages} {pluralRu(summary().unpricedMessages, "ответ", "ответа", "ответов")}</p>
            </Show>
            <Show when={summary().byProvider.length > 0}>
              <table class="data-table">
                <thead><tr><th>Провайдер</th><th>Токены</th><th>Сумма</th></tr></thead>
                <tbody>
                  <For each={summary().byProvider}>
                    {(row) => (
                      <tr>
                        <td>{providerName(row.provider)}</td>
                        <td>{formatTokens(row.inputTokens + row.outputTokens)}</td>
                        <td>{formatCost(row.cost)}</td>
                      </tr>
                    )}
                  </For>
                </tbody>
              </table>
            </Show>
          </>
        )}
      </Show>

      <label class="field-row">
        <span>Лимит на месяц, $</span>
        <input
          id="budget-input"
          class="text-input short-input"
          inputmode="decimal"
          value={budgetText()}
          onInput={(event) => setBudgetText(event.currentTarget.value)}
          onBlur={() => void saveBudget()}
        />
      </label>

      <PriceEditor prices={prices() ?? []} onChange={() => void refetchPrices()} />
    </div>
  );
}

function PriceEditor(props: { prices: PriceEntry[]; onChange: () => void }) {
  const [model, setModel] = createSignal("");
  const [inputPrice, setInputPrice] = createSignal("");
  const [outputPrice, setOutputPrice] = createSignal("");
  const [filter, setFilter] = createSignal("");

  const parsePrice = (text: string) => Number.parseFloat(text.replace(",", "."));

  const visible = createMemo(() => {
    const needle = filter().toLowerCase();
    return props.prices.filter((price) => price.model.toLowerCase().includes(needle)).slice(0, 40);
  });

  const save = async (event: Event) => {
    event.preventDefault();
    const input = parsePrice(inputPrice());
    const output = parsePrice(outputPrice());
    if (!model().trim() || Number.isNaN(input) || Number.isNaN(output)) {
      showToast("Укажи модель и обе цены числами", "error");
      return;
    }
    await api.setPrice(model().trim(), input, output);
    setModel("");
    setInputPrice("");
    setOutputPrice("");
    props.onChange();
  };

  return (
    <section class="price-editor">
      <h3>Цены за миллион токенов</h3>
      <form class="price-form" onSubmit={(event) => void save(event)}>
        <input id="price-model" class="text-input" placeholder="ID модели" aria-label="ID модели" value={model()} onInput={(event) => setModel(event.currentTarget.value)} />
        <input id="price-input" class="text-input short-input" placeholder="вход" aria-label="Цена входа" value={inputPrice()} onInput={(event) => setInputPrice(event.currentTarget.value)} />
        <input id="price-output" class="text-input short-input" placeholder="выход" aria-label="Цена выхода" value={outputPrice()} onInput={(event) => setOutputPrice(event.currentTarget.value)} />
        <button class="ghost-button" type="submit">Сохранить</button>
      </form>
      <input id="price-filter" class="text-input" placeholder="Найти цену" aria-label="Найти цену" value={filter()} onInput={(event) => setFilter(event.currentTarget.value)} />
      <table class="data-table">
        <thead><tr><th>Модель</th><th>Вход</th><th>Выход</th></tr></thead>
        <tbody>
          <For each={visible()}>
            {(price) => (
              <tr>
                <td><code>{price.model}</code></td>
                <td>${price.inputPerMillion.toFixed(2)}</td>
                <td>${price.outputPerMillion.toFixed(2)}</td>
              </tr>
            )}
          </For>
        </tbody>
      </table>
    </section>
  );
}

function LogTab() {
  const [entries, { refetch }] = createResource(() => api.requestLog());

  const clear = async () => {
    await api.clearRequestLog();
    await refetch();
  };

  return (
    <div class="settings-page">
      <h2>Журнал</h2>
      <Show when={(entries() ?? []).length > 0} fallback={<p class="history-empty">Запросов пока не было</p>}>
        <div class="table-scroll">
          <table class="data-table">
            <thead><tr><th>Когда</th><th>Куда</th><th>Модель</th><th>Отправлено</th><th>Получено</th><th>Сообщений</th><th>Файлы</th><th>Итог</th></tr></thead>
            <tbody>
              <For each={entries()}>
                {(entry) => (
                  <tr>
                    <td>{formatRelativeDay(entry.createdAt) === formatClock(entry.createdAt) ? formatClock(entry.createdAt) : `${formatRelativeDay(entry.createdAt)} ${formatClock(entry.createdAt)}`}</td>
                    <td><code>{entry.host}</code></td>
                    <td>{entry.model}</td>
                    <td>{formatBytes(entry.bytesSent)}</td>
                    <td>{formatBytes(entry.bytesReceived)}</td>
                    <td>{entry.messageCount}</td>
                    <td>{entry.files || "нет"}</td>
                    <td>{entry.status}</td>
                  </tr>
                )}
              </For>
            </tbody>
          </table>
        </div>
        <div class="settings-actions">
          <button class="ghost-button" onClick={() => void clear()}>Очистить журнал</button>
        </div>
      </Show>
    </div>
  );
}

function GeneralTab() {
  const [systemPrompt, setSystemPrompt] = createSignal(state.settings().systemPrompt ?? "");

  const changeTheme = (value: string) => {
    void updateSetting("theme", value);
    applyTheme(value);
  };

  return (
    <div class="settings-page">
      <h2>Общие</h2>

      <div class="field-row">
        <span>Тема</span>
        <Select
          id="theme-select"
          label="Тема"
          value={state.settings().theme ?? "system"}
          options={[
            { value: "system", label: "Как в системе" },
            { value: "dark", label: "Тёмная" },
            { value: "light", label: "Светлая" },
          ]}
          onChange={changeTheme}
        />
      </div>

      <div class="field-row">
        <span>Запасная модель при сбое</span>
        <Select
          id="fallback-select"
          label="Запасная модель"
          value={state.settings().fallbackModel ?? ""}
          options={[
            { value: "", label: "Не использовать" },
            ...state.models().map((model) => ({ value: modelRefOf(model), label: `${providerName(model.provider)}: ${model.name}` })),
          ]}
          onChange={(value) => void updateSetting("fallbackModel", value)}
        />
      </div>

      <label class="check-row">
        <span>Вызывать окно по Ctrl+Shift+Space</span>
        <input
          id="hotkey-enabled"
          class="toggle"
          type="checkbox"
          checked={state.settings().hotkeyEnabled !== "false"}
          onChange={(event) => void updateSetting("hotkeyEnabled", String(event.currentTarget.checked))}
        />
      </label>

      <label class="check-row">
        <span>Не давать компьютеру засыпать, пока модель работает</span>
        <input
          id="keep-awake"
          class="toggle"
          type="checkbox"
          checked={state.settings().keepAwake !== "false"}
          onChange={(event) => void updateSetting("keepAwake", String(event.currentTarget.checked))}
        />
      </label>

      <label class="check-row">
        <span>При закрытии сворачивать в трей</span>
        <input
          id="close-to-tray"
          class="toggle"
          type="checkbox"
          checked={state.settings().closeToTray === "true"}
          onChange={(event) => void updateSetting("closeToTray", String(event.currentTarget.checked))}
        />
      </label>

      <label class="field-stack">
        <span>Инструкция для всех чатов</span>
        <textarea
          id="system-prompt"
          class="text-input prompt-input"
          placeholder="Например: отвечай коротко и по-русски"
          value={systemPrompt()}
          onInput={(event) => setSystemPrompt(event.currentTarget.value)}
          onBlur={() => void updateSetting("systemPrompt", systemPrompt())}
        />
      </label>
    </div>
  );
}
