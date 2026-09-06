import { browser } from "$app/environment";
import { startSidebarMotion } from "./sidebar-motion";

const STORAGE_KEY = "kiminola-sidebar-collapsed";

function initialCollapsed(): boolean {
  if (!browser) return false;
  return localStorage.getItem(STORAGE_KEY) === "true";
}

export const sidebarState = $state({ collapsed: initialCollapsed() });

export function toggleSidebar() {
  if (browser) startSidebarMotion();
  sidebarState.collapsed = !sidebarState.collapsed;
  if (browser) localStorage.setItem(STORAGE_KEY, String(sidebarState.collapsed));
}
