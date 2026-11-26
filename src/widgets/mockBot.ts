import type { BotButton, BotScreen, BotSpec } from "./types";

export const START_COMMAND = "/start";
const DEFAULT_FALLBACK = "Не понял. Нажмите /start, чтобы открыть меню.";
const MAX_LOG = 40;
const LOG_TEXT_LENGTH = 40;

export interface MockMessage {
  id: number;
  fromBot: boolean;
  text: string;
  photo?: string;
  buttons: BotButton[][];
  date: number;
  edited: boolean;
}

export interface MockLogEntry {
  method: string;
  detail: string;
}

export interface MockState {
  messages: MockMessage[];
  keyboard: BotButton[][] | null;
  screenId: string | null;
  log: MockLogEntry[];
  nextId: number;
}

export function emptyState(): MockState {
  return { messages: [], keyboard: null, screenId: null, log: [], nextId: 1 };
}

function shortText(text: string): string {
  const firstLine = text.split("\n")[0].trim();
  return firstLine.length > LOG_TEXT_LENGTH ? `«${firstLine.slice(0, LOG_TEXT_LENGTH)}…»` : `«${firstLine}»`;
}

function withLog(state: MockState, method: string, detail: string): MockState {
  return { ...state, log: [{ method, detail }, ...state.log].slice(0, MAX_LOG) };
}

function findScreen(spec: BotSpec, id: string | undefined): BotScreen | undefined {
  return spec.screens.find((screen) => screen.id.trim() === id?.trim());
}

function messageOf(screen: BotScreen, id: number, now: number): MockMessage {
  return {
    id,
    fromBot: true,
    text: screen.text,
    photo: screen.photo,
    buttons: screen.buttons ?? [],
    date: now,
    edited: false,
  };
}

function keyboardAfter(screen: BotScreen, previous: BotButton[][] | null): BotButton[][] | null {
  if (!screen.keyboard) {
    return previous;
  }
  return screen.keyboard.length > 0 ? screen.keyboard : null;
}

function sendScreen(state: MockState, screen: BotScreen, now: number): MockState {
  const message = messageOf(screen, state.nextId, now);
  const next: MockState = {
    ...state,
    messages: [...state.messages, message],
    keyboard: keyboardAfter(screen, state.keyboard),
    screenId: screen.id,
    nextId: state.nextId + 1,
  };
  return withLog(next, screen.photo ? "sendPhoto" : "sendMessage", shortText(screen.text));
}

function editScreen(state: MockState, messageId: number, screen: BotScreen, now: number): MockState {
  const messages = state.messages.map((message) =>
    message.id === messageId ? { ...messageOf(screen, messageId, now), edited: true } : message,
  );
  const next: MockState = { ...state, messages, keyboard: keyboardAfter(screen, state.keyboard), screenId: screen.id };
  return withLog(next, screen.photo ? "editMessageMedia" : "editMessageText", shortText(screen.text));
}

function sendPlain(state: MockState, text: string, now: number): MockState {
  const message: MockMessage = { id: state.nextId, fromBot: true, text, buttons: [], date: now, edited: false };
  const next = { ...state, messages: [...state.messages, message], nextId: state.nextId + 1 };
  return withLog(next, "sendMessage", shortText(text));
}

export function withUserText(state: MockState, text: string, now: number): MockState {
  const message: MockMessage = { id: state.nextId, fromBot: false, text, buttons: [], date: now, edited: false };
  const next = { ...state, messages: [...state.messages, message], nextId: state.nextId + 1 };
  return withLog(next, "message", shortText(text));
}

function keyboardButton(state: MockState, text: string): BotButton | undefined {
  const wanted = text.trim().toLowerCase();
  return state.keyboard?.flat().find((button) => button.text.trim().toLowerCase() === wanted);
}

export function answerToText(spec: BotSpec, state: MockState, text: string, now: number): MockState {
  const start = findScreen(spec, spec.start);
  if (text.trim().split(/\s+/)[0] === START_COMMAND && start) {
    return sendScreen(state, start, now);
  }
  const pressedKey = keyboardButton(state, text);
  const keyTarget = findScreen(spec, pressedKey?.go);
  if (keyTarget) {
    return sendScreen(state, keyTarget, now);
  }
  const current = findScreen(spec, state.screenId ?? undefined);
  const nextScreen = findScreen(spec, current?.next);
  if (nextScreen) {
    return sendScreen(state, nextScreen, now);
  }
  return sendPlain(state, spec.fallback?.trim() || DEFAULT_FALLBACK, now);
}

export function answerToPress(spec: BotSpec, state: MockState, messageId: number, button: BotButton, now: number): MockState {
  const pressed = withLog(state, "callback_query", shortText(button.text));
  const target = findScreen(spec, button.go);
  if (!target) {
    return pressed;
  }
  const stillThere = pressed.messages.some((message) => message.id === messageId);
  if (button.edit && stillThere) {
    return editScreen(pressed, messageId, target, now);
  }
  return sendScreen(pressed, target, now);
}

export function startConversation(spec: BotSpec, now: number): MockState {
  const asked = withUserText(emptyState(), START_COMMAND, now);
  return answerToText(spec, asked, START_COMMAND, now);
}
