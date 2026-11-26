import { For, Show, createEffect, createMemo, createSignal, on } from "solid-js";

import type { ToolCallPart } from "../../api";
import { chooseTerminalMode, clearMyTerminal, panels, runInMyTerminal, stopMyTerminal } from "../../panels";
import { state } from "../../store";

const COMMAND_TOOLS = ["run_command", "start_background", "Bash", "shell", "exec_command", "local_shell"];
const NEAR_BOTTOM_PX = 40;

interface ModelCommand {
  id: string;
  command: string;
  folder: string;
  result: string;
  isRunning: boolean;
  isError: boolean;
}

function commandOf(part: ToolCallPart): string {
  try {
    const parsed = JSON.parse(part.input) as Record<string, unknown>;
    const command = parsed.command ?? parsed.cmd;
    if (Array.isArray(command)) {
      return command.join(" ");
    }
    return typeof command === "string" ? command : part.input;
  } catch {
    return part.input;
  }
}

function shortFolder(folder: string): string {
  const pieces = folder.split(/[\\/]/).filter(Boolean);
  return pieces.length > 2 ? `…\\${pieces.slice(-2).join("\\")}` : folder;
}

function keepAtBottom(element: () => HTMLElement | undefined, content: () => unknown) {
  createEffect(on(content, () => {
    const target = element();
    if (!target) {
      return;
    }
    const wasNearBottom = target.scrollHeight - target.scrollTop - target.clientHeight < NEAR_BOTTOM_PX * 4;
    if (wasNearBottom) {
      requestAnimationFrame(() => target.scrollTo({ top: target.scrollHeight }));
    }
  }));
}

function ModelTerminal() {
  let scrollElement: HTMLDivElement | undefined;
  const commands = createMemo<ModelCommand[]>(() =>
    state.thread.messages.flatMap((message) =>
      message.parts
        .filter((part): part is ToolCallPart => part.type === "tool_call" && COMMAND_TOOLS.includes(part.name))
        .map((part) => {
          const id = part.id ?? `${message.id}-${part.name}`;
          const isRunning = message.status === "streaming" && part.status === "running";
          return {
            id,
            command: commandOf(part),
            folder: part.folder ?? "",
            result: part.result ?? "",
            isRunning,
            isError: part.isError === true,
          };
        }),
    ),
  );
  const outputOf = (entry: ModelCommand) => (entry.isRunning ? panels.liveOutput[entry.id] ?? "" : entry.result);
  const runningOutputLength = () => {
    const running = commands().find((entry) => entry.isRunning);
    return running ? outputOf(running).length : 0;
  };
  keepAtBottom(() => scrollElement, () => `${commands().length}:${runningOutputLength()}`);

  return (
    <div class="terminal-screen" ref={scrollElement}>
      <Show when={commands().length > 0} fallback={<p class="terminal-quiet">Модель пока не выполняла команд</p>}>
        <For each={commands()}>
          {(entry) => (
            <div class="terminal-entry">
              <div>
                <span class="terminal-prompt">{shortFolder(entry.folder)}&gt;</span> {entry.command}
              </div>
              <Show when={outputOf(entry)}>
                <div classList={{ "terminal-error": entry.isError }}>{outputOf(entry)}</div>
              </Show>
              <Show when={entry.isRunning}>
                <div class="terminal-quiet"><span class="shimmer">выполняется</span></div>
              </Show>
            </div>
          )}
        </For>
      </Show>
    </div>
  );
}

function MyTerminal() {
  let scrollElement: HTMLDivElement | undefined;
  let inputElement: HTMLInputElement | undefined;
  const [typed, setTyped] = createSignal("");
  const [history, setHistory] = createSignal<string[]>([]);
  const [historyPosition, setHistoryPosition] = createSignal<number | null>(null);
  keepAtBottom(() => scrollElement, () => panels.myTerminal().text.length);

  const run = () => {
    const line = typed().trim();
    if (!line) {
      return;
    }
    setHistory((previous) => [...previous.filter((item) => item !== line), line]);
    setHistoryPosition(null);
    setTyped("");
    void runInMyTerminal(line);
  };

  const walkHistory = (step: number) => {
    const items = history();
    if (items.length === 0) {
      return;
    }
    const current = historyPosition() ?? items.length;
    const next = Math.min(items.length, Math.max(0, current + step));
    setHistoryPosition(next === items.length ? null : next);
    setTyped(items[next] ?? "");
  };

  const handleKeyDown = (event: KeyboardEvent) => {
    if (event.key === "Enter") {
      event.preventDefault();
      run();
    } else if (event.key === "ArrowUp") {
      event.preventDefault();
      walkHistory(-1);
    } else if (event.key === "ArrowDown") {
      event.preventDefault();
      walkHistory(1);
    }
  };

  return (
    <>
      <div class="terminal-screen" ref={scrollElement} onClick={() => window.getSelection()?.isCollapsed && inputElement?.focus()}>
        {panels.myTerminal().text}
      </div>
      <div class="terminal-input">
        <span class="terminal-prompt">{shortFolder(panels.myTerminal().folder || state.chats().find((chat) => chat.id === state.currentChatId())?.cwd || "")}&gt;</span>
        <input
          ref={inputElement}
          value={typed()}
          placeholder="Команда PowerShell"
          aria-label="Команда"
          spellcheck={false}
          onInput={(event) => setTyped(event.currentTarget.value)}
          onKeyDown={handleKeyDown}
        />
      </div>
    </>
  );
}

export function TerminalPane() {
  return (
    <section class="side-pane side-pane-fill">
      <div class="side-subtabs">
        <button class="side-subtab" classList={{ "is-active": panels.terminalMode() === "model" }} onClick={() => chooseTerminalMode("model")}>
          Модель
        </button>
        <button class="side-subtab" classList={{ "is-active": panels.terminalMode() === "mine" }} onClick={() => chooseTerminalMode("mine")}>
          Мой
        </button>
        <span class="side-subtabs-spacer" />
        <Show when={panels.terminalMode() === "mine"}>
          <Show when={panels.myTerminal().isRunning}>
            <button class="side-link-button" onClick={() => void stopMyTerminal()}>Остановить</button>
          </Show>
          <button class="side-link-button" onClick={() => void clearMyTerminal()}>Очистить</button>
        </Show>
      </div>
      <Show when={panels.terminalMode() === "model"} fallback={<MyTerminal />}>
        <ModelTerminal />
      </Show>
    </section>
  );
}
