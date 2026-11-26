import { createSignal } from "solid-js";

export type Theme = "light" | "dark";

const systemDarkQuery = window.matchMedia("(prefers-color-scheme: dark)");
const [chosenTheme, setChosenTheme] = createSignal<string | undefined>(undefined);
const [systemIsDark, setSystemIsDark] = createSignal(systemDarkQuery.matches);

systemDarkQuery.addEventListener("change", (event) => setSystemIsDark(event.matches));

export function rememberChosenTheme(theme: string | undefined): void {
  setChosenTheme(theme);
}

export function effectiveTheme(): Theme {
  const theme = chosenTheme();
  if (theme === "light" || theme === "dark") {
    return theme;
  }
  return systemIsDark() ? "dark" : "light";
}
