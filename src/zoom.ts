import { createSignal } from "solid-js";
import { getCurrentWebview } from "@tauri-apps/api/webview";

import { api } from "./api";

const MIN_ZOOM = 0.5;
const MAX_ZOOM = 2;
const DEFAULT_ZOOM = 1;
const ZOOM_STEP = 0.1;
const WHEEL_DISTANCE_PER_STEP = 90;
const WHEEL_IDLE_RESET_MS = 300;
const SAVE_DELAY_MS = 400;
const BADGE_LIFETIME_MS = 1100;

const [currentZoom, setCurrentZoom] = createSignal(DEFAULT_ZOOM);
const [isZoomBadgeVisible, setIsZoomBadgeVisible] = createSignal(false);

export const zoomPercent = () => Math.round(currentZoom() * 100);
export const zoomFactor = currentZoom;
export { isZoomBadgeVisible };

let accumulatedWheelDistance = 0;
let wheelResetTimer = 0;
let saveTimer = 0;
let badgeTimer = 0;

function limitZoom(value: number): number {
  const rounded = Math.round(value * 10) / 10;
  return Math.min(MAX_ZOOM, Math.max(MIN_ZOOM, rounded));
}

function applyZoom(value: number): void {
  const next = limitZoom(value);
  setCurrentZoom(next);
  void getCurrentWebview().setZoom(next);
}

function changeZoomByUser(value: number): void {
  applyZoom(value);
  window.clearTimeout(saveTimer);
  saveTimer = window.setTimeout(() => void api.setSetting("zoom", String(currentZoom())), SAVE_DELAY_MS);
  setIsZoomBadgeVisible(true);
  window.clearTimeout(badgeTimer);
  badgeTimer = window.setTimeout(() => setIsZoomBadgeVisible(false), BADGE_LIFETIME_MS);
}

function handleWheel(event: WheelEvent): void {
  if (!event.ctrlKey) {
    return;
  }
  event.preventDefault();
  accumulatedWheelDistance += event.deltaY * currentZoom();
  window.clearTimeout(wheelResetTimer);
  wheelResetTimer = window.setTimeout(() => {
    accumulatedWheelDistance = 0;
  }, WHEEL_IDLE_RESET_MS);
  const steps = Math.trunc(accumulatedWheelDistance / WHEEL_DISTANCE_PER_STEP);
  if (steps !== 0) {
    accumulatedWheelDistance -= steps * WHEEL_DISTANCE_PER_STEP;
    changeZoomByUser(currentZoom() - steps * ZOOM_STEP);
  }
}

function handleKeyDown(event: KeyboardEvent): void {
  if (!event.ctrlKey) {
    return;
  }
  if (event.key === "=" || event.key === "+") {
    event.preventDefault();
    changeZoomByUser(currentZoom() + ZOOM_STEP);
  } else if (event.key === "-") {
    event.preventDefault();
    changeZoomByUser(currentZoom() - ZOOM_STEP);
  } else if (event.key === "0") {
    event.preventDefault();
    changeZoomByUser(DEFAULT_ZOOM);
  }
}

export function installZoomControls(savedZoom: string | undefined): void {
  const saved = Number(savedZoom);
  if (Number.isFinite(saved) && saved > 0 && saved !== DEFAULT_ZOOM) {
    applyZoom(saved);
  }
  document.addEventListener("wheel", handleWheel, { passive: false });
  document.addEventListener("keydown", handleKeyDown);
}
