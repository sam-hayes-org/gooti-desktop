import { SENSITIVE_KINDS } from '../core/core.service';

/** Event kinds people meet most, in words. Others show their number. */
const KIND_NAMES: Record<number, string> = {
  0: 'Profile',
  1: 'Note',
  3: 'Follow list',
  4: 'Direct message (old)',
  5: 'Deletion',
  6: 'Repost',
  7: 'Reaction',
  14: 'Direct message',
  62: 'Erase account',
  1059: 'Gift wrap',
  9734: 'Zap request',
  10000: 'Mute list',
  10002: 'Relay list',
  10050: 'DM relays',
  13194: 'Wallet info',
  22242: 'Relay sign-in',
  23194: 'Wallet request',
  23195: 'Wallet response',
  24133: 'Remote signing',
  24242: 'Blossom upload',
  27235: 'Web sign-in',
  30023: 'Article',
};

/** "Note (kind 1)", or "Kind 31337". */
export function kindLabel(kind: number | null | undefined): string {
  if (kind === null || kind === undefined) {
    return 'Event';
  }
  return KIND_NAMES[kind] ? `${KIND_NAMES[kind]} (kind ${kind})` : `Kind ${kind}`;
}

export function isSensitive(kind: number | null | undefined): boolean {
  return kind !== null && kind !== undefined && SENSITIVE_KINDS.includes(kind);
}

/** What a request or rule is about, in a few words. */
export function methodLabel(method: string, kind?: number | null): string {
  switch (method) {
    case 'sign_event':
      return `Sign: ${kindLabel(kind)}`;
    case 'nip04_encrypt':
    case 'nip44_encrypt':
      return `Encrypt (${method.slice(0, 5).toUpperCase()})`;
    case 'nip04_decrypt':
    case 'nip44_decrypt':
      return `Decrypt (${method.slice(0, 5).toUpperCase()})`;
    default:
      return method;
  }
}
