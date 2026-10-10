import { browser } from "$app/environment";
import { startSidebarMotion } from "./sidebar-motion";

const STORAGE_KEY = "kiminola-sidebar-collapsed";

function initialCollapsed(): boolean {
  if (!browser) return false;
  return localStorage.getItem(STORAGE_KEY) === "true";
}

// Compact navigation is transient; never overwrite the desktop preference.
export const sidebarState = $state({ collapsed: initialCollapsed(), compactOpen: false });

export function closeCompactSidebar() {
  sidebarState.compactOpen = false;
}

export function toggleSidebar() {
  if (browser) startSidebarMotion();
  sidebarState.collapsed = !sidebarState.collapsed;
  if (browser) localStorage.setItem(STORAGE_KEY, String(sidebarState.collapsed));
}
