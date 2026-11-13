/** Keyboard shortcuts: Ctrl on Linux and Windows, ⌘ on macOS. */
const MAC = typeof navigator !== 'undefined' && /Mac/i.test(navigator.platform);

/** The modifier's name as users see it. */
export const MOD = MAC ? '⌘' : 'Ctrl';

/** Ctrl (or ⌘ on macOS) is held, without Alt. */
export function withMod(event: KeyboardEvent): boolean {
  return (MAC ? event.metaKey : event.ctrlKey) && !event.altKey;
}

/** The shortcuts, for the Info page. */
export const SHORTCUTS: readonly { keys: string; does: string }[] = [
  { keys: `${MOD}+1 … ${MOD}+6`, does: 'Open Identities, Connections, Activities, Backup, Settings, Info' },
  { keys: `${MOD}+L`, does: 'Lock Gooti' },
  { keys: `${MOD}+Enter`, does: 'Approve, in the request window' },
  { keys: `${MOD}+Shift+Enter`, does: 'Reject, in the request window' },
];
