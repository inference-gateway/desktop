// Sidebar width: clamping + localStorage persistence for the resizable sidebar.
export const DEFAULT_SIDEBAR_WIDTH = 320;
export const MIN_SIDEBAR_WIDTH = 240;
const SIDEBAR_WIDTH_KEY = "sidebarWidth";

export function clampSidebarWidth(width: number, windowWidth: number): number {
  const max = Math.max(MIN_SIDEBAR_WIDTH, Math.floor(windowWidth / 2));
  return Math.min(Math.max(Math.round(width), MIN_SIDEBAR_WIDTH), max);
}

export function loadSidebarWidth(windowWidth: number): number {
  const stored = Number(localStorage.getItem(SIDEBAR_WIDTH_KEY));
  return Number.isFinite(stored) && stored > 0 ? clampSidebarWidth(stored, windowWidth) : DEFAULT_SIDEBAR_WIDTH;
}

export function saveSidebarWidth(width: number): void {
  localStorage.setItem(SIDEBAR_WIDTH_KEY, String(width));
}
