import { For, Show, createMemo, createSignal, onCleanup, onMount } from "solid-js";

import type { ModelInfo } from "../api";
import { createPresence } from "../motion";
import { actions, clearCompareModels, focusComposer, modelDisplayName, modelRefOf, providerName, state, toggleCompareModel } from "../store";
import { CheckIcon, ChevronDownIcon, CompareIcon } from "./icons";

interface ModelGroup {
  provider: string;
  models: ModelInfo[];
}

export function ModelPicker(props: { onSelect: (modelRef: string) => void }) {
  let rootElement: HTMLDivElement | undefined;
  let searchElement: HTMLInputElement | undefined;
  let buttonElement: HTMLButtonElement | undefined;
  const [isOpen, setIsOpen] = createSignal(false);
  const [query, setQuery] = createSignal("");
  const [compareMode, setCompareMode] = createSignal(false);
  const [activeIndex, setActiveIndex] = createSignal(0);
  const presence = createPresence(isOpen);

  const groups = createMemo<ModelGroup[]>(() => {
    const needle = query().trim().toLowerCase();
    const grouped = new Map<string, ModelInfo[]>();
    for (const model of state.models()) {
      const haystack = `${model.id} ${model.name} ${providerName(model.provider)}`.toLowerCase();
      if (needle && !haystack.includes(needle)) {
        continue;
      }
      const list = grouped.get(model.provider) ?? [];
      list.push(model);
      grouped.set(model.provider, list);
    }
    return [...grouped.entries()].map(([provider, models]) => ({ provider, models }));
  });

  const buttonLabel = createMemo(() => {
    const compared = state.compareModels();
    if (compared.length > 1) {
      return `Сравнить ${compared.length} модели`;
    }
    return modelDisplayName(state.selectedModel());
  });

  const toggle = () => {
    const next = !isOpen();
    setIsOpen(next);
    if (next) {
      setQuery("");
      setActiveIndex(Math.max(0, flatRows().indexOf(state.selectedModel() ?? "")));
      queueMicrotask(() => searchElement?.focus());
    }
  };

  const chooseModel = (modelRef: string) => {
    if (compareMode()) {
      toggleCompareModel(modelRef);
      return;
    }
    clearCompareModels();
    props.onSelect(modelRef);
    setIsOpen(false);
    focusComposer();
  };

  const toggleCompareMode = () => {
    const next = !compareMode();
    setCompareMode(next);
    if (!next) {
      clearCompareModels();
    }
  };

  const closeOnOutsideClick = (event: MouseEvent) => {
    if (isOpen() && rootElement && !rootElement.contains(event.target as Node)) {
      setIsOpen(false);
    }
  };

  const flatRows = createMemo(() => groups().flatMap((group) => group.models.map(modelRefOf)));

  const moveActive = (offset: number) => {
    const lastIndex = flatRows().length - 1;
    const next = Math.min(lastIndex, Math.max(0, activeIndex() + offset));
    setActiveIndex(next);
    rootElement?.querySelector(`[data-row="${next}"]`)?.scrollIntoView({ block: "nearest" });
  };

  const handleMenuKeys = (event: KeyboardEvent) => {
    if (!isOpen()) {
      return;
    }
    if (event.key === "Escape") {
      event.preventDefault();
      setIsOpen(false);
      buttonElement?.focus();
    } else if (event.key === "ArrowDown") {
      event.preventDefault();
      moveActive(1);
    } else if (event.key === "ArrowUp") {
      event.preventDefault();
      moveActive(-1);
    } else if (event.key === "Enter" && (event.target as HTMLElement).id === "model-search") {
      event.preventDefault();
      const reference = flatRows()[activeIndex()];
      if (reference) {
        chooseModel(reference);
      }
    }
  };

  onMount(() => document.addEventListener("mousedown", closeOnOutsideClick));
  onCleanup(() => document.removeEventListener("mousedown", closeOnOutsideClick));

  return (
    <div class="model-picker" ref={rootElement} onKeyDown={handleMenuKeys}>
      <button class="model-button" ref={buttonElement} aria-haspopup="listbox" aria-expanded={isOpen()} onClick={toggle}>
        <span class="model-button-label">{buttonLabel()}</span>
        <ChevronDownIcon />
      </button>

      <Show when={presence.mounted()}>
        <div class="model-menu" classList={{ "is-closing": presence.closing() }}>
          <Show
            when={state.models().length > 0}
            fallback={
              <div class="model-empty">
                <p>Моделей пока нет</p>
                <button
                  class="primary-button"
                  onClick={() => {
                    setIsOpen(false);
                    actions.setSettingsTab("keys");
                  }}
                >
                  Добавить ключ
                </button>
              </div>
            }
          >
            <div class="model-search-row">
              <input
                ref={searchElement}
                id="model-search"
                class="model-search"
                placeholder="Найти модель"
                aria-label="Найти модель"
                role="combobox"
                aria-expanded="true"
                aria-controls="model-list"
                aria-activedescendant={flatRows().length > 0 ? `model-row-${activeIndex()}` : undefined}
                value={query()}
                onInput={(event) => {
                  setQuery(event.currentTarget.value);
                  setActiveIndex(0);
                }}
              />
              <button
                class="icon-button"
                classList={{ "is-active": compareMode() }}
                title="Сравнить до трёх моделей"
                aria-label="Режим сравнения моделей"
                aria-pressed={compareMode()}
                onClick={toggleCompareMode}
              >
                <CompareIcon />
              </button>
            </div>
            <div class="model-list" id="model-list" role="listbox" aria-label="Модели">
              <For each={groups()} fallback={<div class="model-none">Ничего не найдено</div>}>
                {(group) => (
                  <div class="model-group">
                    <div class="section-label">{providerName(group.provider)}</div>
                    <For each={group.models}>
                      {(model) => {
                        const reference = modelRefOf(model);
                        const rowIndex = () => flatRows().indexOf(reference);
                        const isChecked = () =>
                          compareMode() ? state.compareModels().includes(reference) : state.selectedModel() === reference;
                        return (
                          <button
                            id={`model-row-${rowIndex()}`}
                            class="model-row"
                            classList={{ "is-active": activeIndex() === rowIndex() }}
                            data-row={rowIndex()}
                            role="option"
                            aria-selected={activeIndex() === rowIndex()}
                            aria-checked={isChecked()}
                            onMouseMove={() => setActiveIndex(rowIndex())}
                            onClick={() => chooseModel(reference)}
                          >
                            <span class="model-row-name">{model.name}</span>
                            <Show when={isChecked()}>
                              <span class="model-row-check"><CheckIcon /></span>
                            </Show>
                          </button>
                        );
                      }}
                    </For>
                  </div>
                )}
              </For>
            </div>
          </Show>
        </div>
      </Show>
    </div>
  );
}
