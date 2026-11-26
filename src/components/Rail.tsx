import { actions, goToStart, state, toggleHistory } from "../store";
import { ChatsIcon, PlusIcon, SearchIcon, SettingsIcon } from "./icons";

export function Rail() {
  return (
    <nav class="rail" aria-label="Разделы">
      <button class="rail-button" data-hint="Новый чат · Ctrl+N" aria-label="Новый чат" onClick={goToStart}>
        <PlusIcon />
      </button>
      <button
        class="rail-button"
        classList={{ "is-active": state.historyOpen() }}
        data-hint="Чаты · Ctrl+B"
        aria-label="Чаты"
        aria-pressed={state.historyOpen()}
        onClick={toggleHistory}
      >
        <ChatsIcon />
      </button>
      <button class="rail-button" data-hint="Поиск · Ctrl+K" aria-label="Поиск" onClick={() => actions.setPaletteOpen(true)}>
        <SearchIcon />
      </button>
      <div class="rail-spacer" />
      <button class="rail-button" data-hint="Настройки · Ctrl+," aria-label="Настройки" onClick={() => actions.setSettingsTab("keys")}>
        <SettingsIcon />
      </button>
    </nav>
  );
}
