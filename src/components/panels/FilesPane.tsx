import { For, Show, createMemo, createResource, createSignal } from "solid-js";

import { api, type ChangedFile, type DiffLine, type FolderEntry } from "../../api";
import { pluralRu } from "../../format";
import { panels, refreshChangedFiles } from "../../panels";
import { currentChat, state } from "../../store";
import { ChevronDownIcon, ChevronIcon, FileIcon } from "../icons";

const CONTEXT_LINES = 3;

type ShownLine = DiffLine | { kind: "gap"; count: number };

export function collapseUnchanged(lines: DiffLine[]): ShownLine[] {
  const isNearChange = lines.map((_, index) => {
    const from = Math.max(0, index - CONTEXT_LINES);
    const to = Math.min(lines.length - 1, index + CONTEXT_LINES);
    for (let near = from; near <= to; near += 1) {
      if (lines[near].kind !== "same") {
        return true;
      }
    }
    return false;
  });
  const shown: ShownLine[] = [];
  let hidden = 0;
  lines.forEach((line, index) => {
    if (isNearChange[index]) {
      if (hidden > 0) {
        shown.push({ kind: "gap", count: hidden });
        hidden = 0;
      }
      shown.push(line);
    } else {
      hidden += 1;
    }
  });
  if (hidden > 0) {
    shown.push({ kind: "gap", count: hidden });
  }
  return shown;
}

const LINE_MARKS = { same: " ", added: "+", removed: "−" } as const;

function DiffView(props: { lines: DiffLine[] }) {
  const shown = createMemo(() => collapseUnchanged(props.lines));
  return (
    <div class="code-view">
      <For each={shown()}>
        {(line) =>
          line.kind === "gap" ? (
            <div class="code-gap">{line.count} {pluralRu(line.count, "строка", "строки", "строк")} без изменений</div>
          ) : (
            <div class={`code-line is-${line.kind}`}>{LINE_MARKS[line.kind]} {line.text}</div>
          )
        }
      </For>
    </div>
  );
}

function FolderLevel(props: { relative: string; depth: number; selected: string | null; onSelect: (path: string) => void }) {
  const [entries] = createResource(
    () => ({ chatId: state.currentChatId(), relative: props.relative }),
    ({ chatId, relative }) => (chatId ? api.listFolder(chatId, relative) : Promise.resolve([] as FolderEntry[])),
    { initialValue: [] },
  );
  return (
    <For each={entries()}>
      {(entry) => <FolderRow entry={entry} depth={props.depth} selected={props.selected} onSelect={props.onSelect} />}
    </For>
  );
}

function FolderRow(props: { entry: FolderEntry; depth: number; selected: string | null; onSelect: (path: string) => void }) {
  const [isOpen, setIsOpen] = createSignal(false);
  const indent = () => ({ "padding-left": `${8 + props.depth * 14}px` });
  return (
    <>
      <button
        class="tree-row"
        classList={{ "is-folder": props.entry.isFolder, "is-active": props.selected === props.entry.relativePath }}
        style={indent()}
        aria-expanded={props.entry.isFolder ? isOpen() : undefined}
        onClick={() => (props.entry.isFolder ? setIsOpen(!isOpen()) : props.onSelect(props.entry.relativePath))}
      >
        <Show when={props.entry.isFolder} fallback={<FileIcon />}>
          {isOpen() ? <ChevronDownIcon /> : <ChevronIcon />}
        </Show>
        <span class="tree-name">{props.entry.name}</span>
      </button>
      <Show when={props.entry.isFolder && isOpen()}>
        <FolderLevel relative={props.entry.relativePath} depth={props.depth + 1} selected={props.selected} onSelect={props.onSelect} />
      </Show>
    </>
  );
}

function FilePreview(props: { selected: string | null }) {
  const [view] = createResource(
    () => {
      const chatId = state.currentChatId();
      const path = props.selected;
      return chatId && path ? { chatId, path, tick: state.usageTick() } : null;
    },
    ({ chatId, path }) => api.fileView(chatId, path),
  );

  return (
    <Show when={view()}>
      {(current) => (
        <div class="files-preview">
          <div class="files-preview-path" title={current().path}>{current().path}</div>
          <Show when={!current().problem} fallback={<p class="side-empty">{current().problem}</p>}>
            <Show when={current().diff} fallback={<pre class="code-view">{current().text ?? "Файла больше нет"}</pre>}>
              {(diff) => <DiffView lines={diff()} />}
            </Show>
          </Show>
        </div>
      )}
    </Show>
  );
}

function changeText(file: ChangedFile): string {
  if (file.isDeleted) {
    return "удалён";
  }
  if (file.isNew) {
    return "новый";
  }
  return `+${file.added} −${file.removed}`;
}

export function ChangesPane() {
  const [selected, setSelected] = createSignal<string | null>(null);

  void refreshChangedFiles();

  return (
    <section class="side-pane side-pane-fill">
      <div class="files-list">
        <For each={panels.changedFiles()} fallback={<p class="side-empty">В этом чате файлы не менялись</p>}>
          {(file) => (
            <button class="tree-row" classList={{ "is-active": selected() === file.path }} onClick={() => setSelected(file.path)}>
              <FileIcon />
              <span class="tree-name" title={file.path}>{file.relativePath}</span>
              <span class="change-mark" classList={{ "is-new": file.isNew }}>{changeText(file)}</span>
            </button>
          )}
        </For>
      </div>
      <FilePreview selected={selected()} />
    </section>
  );
}

export function FilesPane() {
  const [selected, setSelected] = createSignal<string | null>(null);

  return (
    <section class="side-pane side-pane-fill">
      <div class="files-list">
        <Show when={currentChat()?.cwd} fallback={<p class="side-empty">У чата нет рабочей папки</p>}>
          {(folder) => (
            <>
              <h2 class="side-section-title files-heading" title={folder()}>{folder()}</h2>
              <FolderLevel relative="" depth={0} selected={selected()} onSelect={setSelected} />
            </>
          )}
        </Show>
      </div>
      <FilePreview selected={selected()} />
    </section>
  );
}
