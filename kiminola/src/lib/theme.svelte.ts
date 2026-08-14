import { browser } from "$app/environment";

export type Theme = "light" | "dark";

const STORAGE_KEY = "kiminola-theme";

function initialTheme(): Theme {
  if (!browser) return "light";
  const saved = localStorage.getItem(STORAGE_KEY);
  if (saved === "light" || saved === "dark") return saved;
  return window.matchMedia?.("(prefers-color-scheme: dark)").matches ? "dark" : "light";
}

export const themeState = $state<{ theme: Theme }>({ theme: initialTheme() });

export function toggleTheme() {
  themeState.theme = themeState.theme === "dark" ? "light" : "dark";
  if (browser) localStorage.setItem(STORAGE_KEY, themeState.theme);
}
