import { For, Show, createMemo, createResource, createSignal, onCleanup, onMount } from "solid-js";
import { listen } from "@tauri-apps/api/event";
import { open } from "@tauri-apps/plugin-dialog";

import { api, type McpServerStatus, type Plugin, type Skill, type VoiceProgress, type VoiceStatus } from "../api";
import { formatBytes, pluralRu } from "../format";
import { showToast, state, updateSetting } from "../store";
import { TrashIcon } from "./icons";

const ACCESS_SWITCHES = [
  { key: "toolsRead", title: "Читать файлы" },
  { key: "toolsEdit", title: "Менять файлы" },
  { key: "toolsCommand", title: "Запускать команды" },
];

const VOICE_MODEL_MEGABYTES = 396;
const VOICE_ENGINE_MEGABYTES = 8;

function SwitchRow(props: { id: string; title: string; hint?: string; source?: string; checked: boolean; onChange: (checked: boolean) => void }) {
  return (
    <label class="settings-item">
      <span class="settings-item-text">
        <strong>{props.title}</strong>
        <Show when={props.hint}><small>{props.hint}</small></Show>
      </span>
      <Show when={props.source}><span class="settings-item-source">{props.source}</span></Show>
      <input id={props.id} class="toggle" type="checkbox" checked={props.checked} onChange={(event) => props.onChange(event.currentTarget.checked)} />
    </label>
  );
}

export function AccessTab() {
  return (
    <div class="settings-page">
      <h2>Доступ</h2>
      <div class="settings-list">
        <For each={ACCESS_SWITCHES}>
          {(entry) => (
            <SwitchRow
              id={`access-${entry.key}`}
              title={entry.title}
              checked={state.settings()[entry.key] !== "false"}
              onChange={(checked) => void updateSetting(entry.key, String(checked))}
            />
          )}
        </For>
        <SwitchRow
          id="access-remember"
          title="Запоминать режим для новых чатов"
          checked={state.settings().rememberAccess === "true"}
          onChange={(checked) => void updateSetting("rememberAccess", String(checked))}
        />
      </div>
    </div>
  );
}

export function SkillsTab() {
  const [skills, { refetch }] = createResource(() => api.listSkills());

  const toggle = async (skill: Skill, enabled: boolean) => {
    await api.setSkillEnabled(skill.id, enabled);
    await refetch();
  };

  const addFolder = async () => {
    const folder = await open({ directory: true, title: "Папка со скиллами" });
    if (typeof folder !== "string") {
      return;
    }
    try {
      const count = await api.addSkillFolder(folder);
      showToast(`Добавлено: ${count} ${pluralRu(count, "скилл", "скилла", "скиллов")}`);
      await refetch();
    } catch (error) {
      showToast(String(error), "error");
    }
  };

  return (
    <div class="settings-page">
      <h2>Скиллы</h2>
      <div class="settings-list" id="skills-list">
        <For each={skills() ?? []} fallback={<p class="settings-empty">Скиллов пока нет</p>}>
          {(skill) => (
            <SwitchRow
              id={`skill-${skill.id}`}
              title={skill.name}
              hint={skill.description}
              source={skill.source}
              checked={skill.enabled}
              onChange={(enabled) => void toggle(skill, enabled)}
            />
          )}
        </For>
      </div>
      <div class="settings-buttons">
        <button class="ghost-button" onClick={() => void addFolder()}>Добавить папку со скиллами</button>
      </div>
    </div>
  );
}

function ServerRow(props: { server: McpServerStatus; onChanged: () => void }) {
  const [statusText, setStatusText] = createSignal<string | null>(null);
  const [isChecking, setIsChecking] = createSignal(false);

  const check = async () => {
    setIsChecking(true);
    try {
      const count = await api.checkMcpServer(props.server.name);
      setStatusText(`Работает: ${count} ${pluralRu(count, "инструмент", "инструмента", "инструментов")}`);
    } catch (error) {
      setStatusText(String(error));
    } finally {
      setIsChecking(false);
    }
  };

  const setEnabled = async (enabled: boolean) => {
    await api.setMcpServerEnabled(props.server.name, enabled);
    props.onChanged();
  };

  const remove = async () => {
    await api.removeMcpServer(props.server.name);
    props.onChanged();
  };

  return (
    <div class="settings-item">
      <span class="settings-item-text">
        <strong>{props.server.name}</strong>
        <small>{statusText() ?? props.server.target}</small>
      </span>
      <span class="settings-item-source">{props.server.source}</span>
      <button class="link-button" disabled={isChecking()} onClick={() => void check()}>{isChecking() ? "Проверяю…" : "Проверить"}</button>
      <Show when={props.server.source === "Вручную"}>
        <button class="action-button" title="Удалить сервер" aria-label={`Удалить ${props.server.name}`} onClick={() => void remove()}>
          <TrashIcon />
        </button>
      </Show>
      <input class="toggle" type="checkbox" aria-label={`Включить ${props.server.name}`} checked={props.server.enabled} onChange={(event) => void setEnabled(event.currentTarget.checked)} />
    </div>
  );
}

function pluginHint(plugin: Plugin): string {
  const pieces = [];
  if (plugin.skillCount > 0) {
    pieces.push(`${plugin.skillCount} ${pluralRu(plugin.skillCount, "скилл", "скилла", "скиллов")}`);
  }
  if (plugin.serverNames.length > 0) {
    pieces.push(`сервер ${plugin.serverNames.join(", ")}`);
  }
  return [pieces.join(", "), plugin.description].filter(Boolean).join(". ");
}

export function PluginsTab() {
  const [servers, { refetch: refetchServers }] = createResource(() => api.listMcpServers());
  const [plugins, { refetch: refetchPlugins }] = createResource(() => api.listPlugins());
  const [isCatalogOpen, setIsCatalogOpen] = createSignal(false);
  const [isFormOpen, setIsFormOpen] = createSignal(false);
  const [serverName, setServerName] = createSignal("");
  const [serverTarget, setServerTarget] = createSignal("");
  const [filter, setFilter] = createSignal("");

  const visiblePlugins = createMemo(() => {
    const needle = filter().trim().toLowerCase();
    return (plugins() ?? []).filter((plugin) => `${plugin.name} ${plugin.description}`.toLowerCase().includes(needle));
  });

  const addServer = async (event: Event) => {
    event.preventDefault();
    try {
      await api.addMcpServer(serverName(), serverTarget());
      setServerName("");
      setServerTarget("");
      setIsFormOpen(false);
      await refetchServers();
    } catch (error) {
      showToast(String(error), "error");
    }
  };

  const togglePlugin = async (plugin: Plugin, enabled: boolean) => {
    await api.setPluginEnabled(plugin.id, enabled);
    await Promise.all([refetchPlugins(), refetchServers()]);
  };

  return (
    <div class="settings-page">
      <h2>Плагины и серверы MCP</h2>
      <div class="settings-list" id="mcp-list">
        <For each={servers() ?? []} fallback={<p class="settings-empty">Серверов пока нет</p>}>
          {(server) => <ServerRow server={server} onChanged={() => void refetchServers()} />}
        </For>
      </div>
      <Show when={isFormOpen()}>
        <form class="settings-inline-form" onSubmit={(event) => void addServer(event)}>
          <input id="mcp-name" class="text-input" placeholder="Название" aria-label="Название сервера" value={serverName()} onInput={(event) => setServerName(event.currentTarget.value)} />
          <input id="mcp-target" class="text-input" placeholder="Команда запуска или адрес https://" aria-label="Команда или адрес" value={serverTarget()} onInput={(event) => setServerTarget(event.currentTarget.value)} />
          <button class="primary-button" type="submit">Подключить</button>
        </form>
      </Show>
      <div class="settings-buttons">
        <button class="ghost-button" onClick={() => setIsFormOpen(!isFormOpen())}>Подключить сервер MCP</button>
        <button class="ghost-button" aria-expanded={isCatalogOpen()} onClick={() => setIsCatalogOpen(!isCatalogOpen())}>Каталог плагинов</button>
      </div>
      <Show when={isCatalogOpen()}>
        <input id="plugin-filter" class="text-input" placeholder="Найти плагин" aria-label="Найти плагин" value={filter()} onInput={(event) => setFilter(event.currentTarget.value)} />
        <div class="settings-list catalog-list" id="plugin-list">
          <For each={visiblePlugins()} fallback={<p class="settings-empty">Каталог не найден</p>}>
            {(plugin) => (
              <SwitchRow
                id={`plugin-${plugin.id}`}
                title={plugin.name}
                hint={pluginHint(plugin)}
                checked={plugin.enabled}
                onChange={(enabled) => void togglePlugin(plugin, enabled)}
              />
            )}
          </For>
        </div>
      </Show>
    </div>
  );
}

export function VoiceTab() {
  const [status, { mutate }] = createResource<VoiceStatus>(() => api.voiceStatus());
  const [progress, setProgress] = createSignal<VoiceProgress | null>(null);
  const [isInstalling, setIsInstalling] = createSignal(false);
  const [replacementsText, setReplacementsText] = createSignal(state.settings().voiceReplacements ?? "");
  let stopListening: (() => void) | undefined;

  onMount(() => {
    void listen<VoiceProgress>("voice-progress", (event) => setProgress(event.payload)).then((stop) => {
      stopListening = stop;
    });
  });
  onCleanup(() => stopListening?.());

  const install = async () => {
    setIsInstalling(true);
    setProgress(null);
    try {
      mutate(await api.installVoice());
      showToast("Распознавание речи готово");
    } catch (error) {
      showToast(String(error), "error");
    } finally {
      setIsInstalling(false);
      setProgress(null);
    }
  };

  const progressShare = () => {
    const current = progress();
    return current && current.totalBytes > 0 ? current.doneBytes / current.totalBytes : 0;
  };

  const downloadLabel = () => {
    const engineMegabytes = status()?.isEngineInstalled ? 0 : VOICE_ENGINE_MEGABYTES;
    return `Скачать, ${VOICE_MODEL_MEGABYTES + engineMegabytes} МБ`;
  };

  return (
    <div class="settings-page">
      <h2>Голос</h2>
      <Show when={progress()}>
        {(current) => (
          <div class="progress-block" role="status">
            <div class="progress-track"><div class="progress-fill" style={{ transform: `scaleX(${progressShare()})` }} /></div>
            <span class="progress-text">
              {current().stage === "engine" ? "Программа распознавания" : "Модель"}: {formatBytes(current().doneBytes)} из {formatBytes(current().totalBytes)}
            </span>
          </div>
        )}
      </Show>
      <Show when={status()?.isReady !== true} fallback={<p class="settings-note">Готово</p>}>
        <div class="settings-buttons">
          <button class="primary-button" disabled={isInstalling()} onClick={() => void install()}>
            {isInstalling() ? "Скачиваю…" : downloadLabel()}
          </button>
        </div>
      </Show>
      <label class="field-stack">
        <span>Свои замены, по одной в строке</span>
        <textarea
          id="voice-replacements"
          class="text-input prompt-input"
          placeholder={"код-код = Claude Code\nпарсор = парсер"}
          value={replacementsText()}
          onInput={(event) => setReplacementsText(event.currentTarget.value)}
          onBlur={() => void updateSetting("voiceReplacements", replacementsText())}
        />
      </label>
    </div>
  );
}
