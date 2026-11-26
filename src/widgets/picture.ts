import { save } from "@tauri-apps/plugin-dialog";

import { api } from "../api";
import { showToast } from "../store";
import { readViewBox } from "./drawing";

const TARGET_WIDTH = 1600;
const MAX_SCALE = 4;
const FALLBACK_SIZE = { width: 800, height: 600 };

async function pngFromSvg(dataUrl: string, svg: string): Promise<string> {
  const image = new Image();
  image.src = dataUrl;
  await image.decode();
  const box = readViewBox(svg) ?? FALLBACK_SIZE;
  const scale = Math.min(MAX_SCALE, Math.max(1, TARGET_WIDTH / box.width));
  const canvas = document.createElement("canvas");
  canvas.width = Math.round(box.width * scale);
  canvas.height = Math.round(box.height * scale);
  const context = canvas.getContext("2d");
  if (!context) {
    throw new Error("Рисование недоступно");
  }
  context.fillStyle = "#ffffff";
  context.fillRect(0, 0, canvas.width, canvas.height);
  context.drawImage(image, 0, 0, canvas.width, canvas.height);
  return canvas.toDataURL("image/png").split(",")[1] ?? "";
}

export function safeFileName(title: string): string {
  return title.replace(/[\\/:*?"<>|]/g, " ").trim() || "рисунок";
}

export async function savePicture(title: string, dataUrl: string, svg: string): Promise<void> {
  const target = await save({ title: "Сохранить картинкой", defaultPath: `${safeFileName(title)}.png`, filters: [{ name: "PNG", extensions: ["png"] }] });
  if (!target) {
    return;
  }
  try {
    await api.saveWidgetPicture(target, await pngFromSvg(dataUrl, svg));
    showToast("Картинка сохранена");
  } catch (error) {
    showToast(String(error), "error");
  }
}
