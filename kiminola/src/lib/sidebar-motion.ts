// Geometry animates only for an explicit sidebar toggle, never for resizing.
let timeout: ReturnType<typeof setTimeout> | undefined;

export function stopSidebarMotion() {
  clearTimeout(timeout);
  timeout = undefined;
  delete document.documentElement.dataset.sidebarMotion;
}

export function startSidebarMotion() {
  stopSidebarMotion();
  if (window.matchMedia("(prefers-reduced-motion: reduce), (max-width: 760px)").matches) return;
  const root = document.documentElement;
  root.dataset.sidebarMotion = "true";
  // Commit the transition duration before the Svelte state changes its target.
  getComputedStyle(root).getPropertyValue("--sidebar-width");
  timeout = setTimeout(stopSidebarMotion, 200);
}
