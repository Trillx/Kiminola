import { browser } from "$app/environment";

const STORAGE_KEY = "kiminola-sidebar-collapsed";

function initialCollapsed(): boolean {
  if (!browser) return false;
  return localStorage.getItem(STORAGE_KEY) === "true";
}

export const sidebarState = $state({ collapsed: initialCollapsed() });

export function toggleSidebar() {
  sidebarState.collapsed = !sidebarState.collapsed;
  if (browser) localStorage.setItem(STORAGE_KEY, String(sidebarState.collapsed));
}
