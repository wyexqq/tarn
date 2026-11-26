import type { SimMessage, TextRun } from "../api";
import type { MockMessage } from "./mockBot";
import type { BotButton } from "./types";

export type ButtonAction = { kind: "callback"; data: string } | { kind: "go"; button: BotButton } | { kind: "url"; url: string } | { kind: "inert" };

export interface BubbleButton {
  text: string;
  action: ButtonAction;
}

export interface BubblePicture {
  caption: string;
  source: string | null;
  isPhoto: boolean;
}

export interface Bubble {
  id: number;
  fromBot: boolean;
  runs: TextRun[];
  picture: BubblePicture | null;
  buttons: BubbleButton[][];
  date: number;
  edited: boolean;
  forwarded: boolean;
}

const PHOTO_KINDS = ["photo", "sticker", "animation"];
const MEDIA_NAMES: Record<string, string> = {
  photo: "Фото",
  document: "Файл",
  video: "Видео",
  animation: "GIF",
  audio: "Аудио",
  voice: "Голосовое",
  sticker: "Стикер",
  video_note: "Кружок",
};

function isWebAddress(url: string | undefined): url is string {
  return typeof url === "string" && /^https?:\/\//i.test(url.trim());
}

function mockButton(button: BotButton): BubbleButton {
  if (isWebAddress(button.url)) {
    return { text: button.text, action: { kind: "url", url: button.url.trim() } };
  }
  if (button.go) {
    return { text: button.text, action: { kind: "go", button } };
  }
  return { text: button.text, action: { kind: "inert" } };
}

export function bubbleFromMock(message: MockMessage): Bubble {
  return {
    id: message.id,
    fromBot: message.fromBot,
    runs: message.text ? [{ text: message.text }] : [],
    picture: message.photo ? { caption: message.photo, source: null, isPhoto: true } : null,
    buttons: message.buttons.map((row) => row.map(mockButton)),
    date: message.date,
    edited: message.edited,
    forwarded: false,
  };
}

export function bubbleFromSimulator(message: SimMessage, fileAddress: (fileId: string) => string): Bubble {
  const media = message.media;
  const picture = media
    ? {
        caption: PHOTO_KINDS.includes(media.kind) && media.hasBytes ? "" : `${MEDIA_NAMES[media.kind] ?? "Вложение"}${media.name ? ` · ${media.name}` : ""}`,
        source: PHOTO_KINDS.includes(media.kind) && media.hasBytes ? fileAddress(media.fileId) : null,
        isPhoto: PHOTO_KINDS.includes(media.kind),
      }
    : null;
  const buttons = message.buttons.map((row) =>
    row.map((button): BubbleButton => {
      if (button.data !== null) {
        return { text: button.text, action: { kind: "callback", data: button.data } };
      }
      if (isWebAddress(button.url ?? undefined)) {
        return { text: button.text, action: { kind: "url", url: (button.url ?? "").trim() } };
      }
      return { text: button.text, action: { kind: "inert" } };
    }),
  );
  return {
    id: message.id,
    fromBot: message.fromBot,
    runs: message.runs,
    picture,
    buttons,
    date: message.date,
    edited: message.edited,
    forwarded: message.forwarded,
  };
}

export function clockOf(date: number): string {
  return new Date(date).toLocaleTimeString("ru-RU", { hour: "2-digit", minute: "2-digit" });
}

export const CONNECTED_WINDOW_MS = 65_000;

export type LinkState = "off" | "waiting" | "connected" | "webhook" | "lost";

export function linkStateOf(running: boolean, lastCallAt: number | null, webhook: string | null, now: number): LinkState {
  if (!running) {
    return "off";
  }
  if (webhook) {
    return "webhook";
  }
  if (lastCallAt === null) {
    return "waiting";
  }
  return now - lastCallAt <= CONNECTED_WINDOW_MS ? "connected" : "lost";
}

export const LINK_TEXT: Record<LinkState, string> = {
  off: "Симулятор выключен",
  waiting: "Жду бота",
  connected: "Бот подключён",
  webhook: "Бот на вебхуке",
  lost: "Бот молчит больше минуты",
};
