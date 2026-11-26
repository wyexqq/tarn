import { For, Show, createSignal } from "solid-js";

import type { BackgroundTask } from "../../api";
import { formatClock } from "../../format";
import { loadTaskOutput, panels, stopTask } from "../../panels";
import { state, updateSetting } from "../../store";

function taskState(task: BackgroundTask): string {
  if (task.finishedAt === null) {
    return "работает";
  }
  if (task.wasStopped) {
    return "остановлена";
  }
  return task.exitCode === 0 ? "готово" : `код ${task.exitCode ?? "?"}`;
}

function TaskRow(props: { task: BackgroundTask }) {
  const [isOpen, setIsOpen] = createSignal(false);
  const isRunning = () => props.task.finishedAt === null;
  const failed = () => !isRunning() && !props.task.wasStopped && props.task.exitCode !== 0;

  const toggle = () => {
    const open = !isOpen();
    setIsOpen(open);
    if (open) {
      void loadTaskOutput(props.task.id);
    }
  };

  return (
    <div class="task-item">
      <div class="task-row">
        <i class="side-dot" classList={{ "is-running": isRunning(), "is-failed": failed(), "is-off": props.task.wasStopped }} />
        <button class="task-main" aria-expanded={isOpen()} onClick={toggle}>
          <span class="task-name">{props.task.name}</span>
          <code class="task-command">{props.task.command}</code>
        </button>
        <Show
          when={isRunning()}
          fallback={<span class="task-time">{taskState(props.task)} · {formatClock(props.task.finishedAt ?? props.task.startedAt)}</span>}
        >
          <button class="ghost-button" onClick={() => void stopTask(props.task.id)}>Остановить</button>
        </Show>
      </div>
      <Show when={isOpen()}>
        <pre class="task-output">{panels.taskOutputs[props.task.id] || "Вывода пока нет"}</pre>
      </Show>
    </div>
  );
}

export function TasksPane() {
  const keepsAwake = () => state.settings().keepAwake !== "false";
  return (
    <section class="side-pane">
      <Show when={panels.tasks().length > 0} fallback={<p class="side-empty">Фоновых задач нет</p>}>
        <For each={panels.tasks()}>{(task) => <TaskRow task={task} />}</For>
      </Show>
      <label class="side-section side-row-between awake-row">
        <span>Не давать компьютеру засыпать, пока модель работает</span>
        <input
          class="toggle"
          type="checkbox"
          checked={keepsAwake()}
          onChange={(event) => void updateSetting("keepAwake", String(event.currentTarget.checked))}
        />
      </label>
    </section>
  );
}
