import { createSignal } from "solid-js";
import { listen } from "@tauri-apps/api/event";

import { api, type SimView } from "../api";

const CHANGED_EVENT = "bot-sim-changed";

const [simulatorView, setSimulatorView] = createSignal<SimView | null>(null);
let subscribed = false;

export function watchSimulator(): () => SimView | null {
  if (!subscribed) {
    subscribed = true;
    void listen<SimView>(CHANGED_EVENT, (event) => setSimulatorView(event.payload));
    void api.botSimView().then(setSimulatorView);
  }
  return simulatorView;
}

export async function startSimulator(): Promise<void> {
  setSimulatorView(await api.botSimStart());
}

export async function stopSimulator(): Promise<void> {
  await api.botSimStop();
  setSimulatorView(await api.botSimView());
}
