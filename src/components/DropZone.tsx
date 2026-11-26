import { Show, createSignal, onCleanup, onMount } from "solid-js";
import { getCurrentWindow } from "@tauri-apps/api/window";

const [pendingFiles, setPendingFiles] = createSignal<string[]>([]);

export const pendingDroppedFiles = pendingFiles;

export function takeDroppedFiles(): string[] {
  const files = pendingFiles();
  setPendingFiles([]);
  return files;
}

export function DropZone() {
  const [isDragging, setIsDragging] = createSignal(false);
  let stopListening: (() => void) | undefined;

  onMount(async () => {
    stopListening = await getCurrentWindow().onDragDropEvent((event) => {
      const kind = event.payload.type;
      if (kind === "enter" || kind === "over") {
        setIsDragging(true);
      } else if (kind === "leave") {
        setIsDragging(false);
      } else if (kind === "drop") {
        setIsDragging(false);
        setPendingFiles(event.payload.paths);
      }
    });
  });
  onCleanup(() => stopListening?.());

  return (
    <Show when={isDragging()}>
      <div class="drop-zone">
        <div class="drop-zone-label">Отпусти, чтобы прикрепить</div>
      </div>
    </Show>
  );
}
