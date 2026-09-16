export const DEFAULT_SIDEBAR_WIDTH = 320;
export const MIN_SIDEBAR_WIDTH = 240;
const SIDEBAR_WIDTH_KEY = "sidebarWidth";

export function clampSidebarWidth(width: number, windowWidth: number): number {
  const max = Math.max(MIN_SIDEBAR_WIDTH, Math.floor(windowWidth / 2));
  return Math.min(Math.max(Math.round(width), MIN_SIDEBAR_WIDTH), max);
}

export function loadSidebarWidth(
  windowWidth: number,
  key = SIDEBAR_WIDTH_KEY,
  fallback = DEFAULT_SIDEBAR_WIDTH,
): number {
  const stored = Number(localStorage.getItem(key));
  return Number.isFinite(stored) && stored > 0 ? clampSidebarWidth(stored, windowWidth) : fallback;
}

export function saveSidebarWidth(width: number, key = SIDEBAR_WIDTH_KEY): void {
  localStorage.setItem(key, String(width));
}
