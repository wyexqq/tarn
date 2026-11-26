import { Menu } from "@tauri-apps/api/menu";
import { getCurrentWindow, ProgressBarStatus, UserAttentionType } from "@tauri-apps/api/window";

export interface ContextMenuAction {
  text: string;
  run: () => void;
}

export type ContextMenuEntry = ContextMenuAction | "separator";

function reportNativeFailure(error: unknown): void {
  console.warn("системная функция недоступна:", error);
}

export function setNativeTheme(theme: string | undefined): void {
  const nativeTheme = theme === "light" || theme === "dark" ? theme : null;
  getCurrentWindow().setTheme(nativeTheme).catch(reportNativeFailure);
}

export function setWindowTitle(chatTitle: string | null): void {
  const title = chatTitle ? `${chatTitle} — Tarn` : "Tarn";
  getCurrentWindow().setTitle(title).catch(reportNativeFailure);
}

export function setTaskbarProgress(share: number | null): void {
  const options =
    share === null
      ? { status: ProgressBarStatus.None }
      : { status: ProgressBarStatus.Normal, progress: Math.round(share * 100) };
  getCurrentWindow().setProgressBar(options).catch(reportNativeFailure);
}

export function flashTaskbarIfInBackground(): void {
  if (!document.hasFocus()) {
    getCurrentWindow().requestUserAttention(UserAttentionType.Informational).catch(reportNativeFailure);
  }
}

export function targetsTextEditing(event: MouseEvent): boolean {
  const isEditable = (event.target as HTMLElement).closest("input, textarea") !== null;
  return isEditable || (window.getSelection()?.toString() ?? "").length > 0;
}

export async function showContextMenu(entries: ContextMenuEntry[]): Promise<void> {
  const items = entries.map((entry) =>
    entry === "separator" ? { item: "Separator" as const } : { text: entry.text, action: entry.run },
  );
  const menu = await Menu.new({ items });
  await menu.popup();
}

export async function showEditingContextMenu(): Promise<void> {
  const menu = await Menu.new({
    items: [
      { item: "Cut", text: "Вырезать" },
      { item: "Copy", text: "Копировать" },
      { item: "Paste", text: "Вставить" },
      { item: "Separator" },
      { item: "SelectAll", text: "Выделить всё" },
    ],
  });
  await menu.popup();
}

export async function showSelectionContextMenu(): Promise<void> {
  const menu = await Menu.new({
    items: [
      { item: "Copy", text: "Копировать" },
      { item: "SelectAll", text: "Выделить всё" },
    ],
  });
  await menu.popup();
}
