import type { ComposerSettings } from "./tauri";

/** The composer's default table and status bar, the CLI's own defaults. They
 * apply only while the resolved config is out of reach - an answer the CLI
 * rejects shows its error and these keep the composer working. */
export const COMPOSER_DEFAULT: ComposerSettings = {
  triggers: {
    chat: { "@": ["files", "agents"], "#": ["issues"] },
    room: { "@": ["agents", "files"], "#": ["issues"] },
  },
  status_bar: { enabled: true, indicators: {} },
};

/** The context's trigger table, the CLI's resolved answer used as is - never
 * merged with built-ins, so a trigger it leaves out of the context opens
 * nothing there. */
export function tableFor(composer: ComposerSettings | null, context: string): Record<string, string[]> {
  return (composer ?? COMPOSER_DEFAULT).triggers[context] ?? {};
}

/** The context's triggers, the one characters a token may open a menu with. */
export function triggersFor(composer: ComposerSettings | null, context: string): string[] {
  return Object.keys(tableFor(composer, context));
}

/** Whether the status bar shows an indicator: a key the config leaves out
 * keeps the CLI's default, which is on. */
export function indicatorOn(composer: ComposerSettings | null, name: string): boolean {
  return composer?.status_bar.indicators[name] ?? true;
}

/** Whether the status bar shows at all, by the switch composer.yaml holds. */
export function statusBarEnabled(composer: ComposerSettings | null): boolean {
  return composer?.status_bar.enabled ?? true;
}
