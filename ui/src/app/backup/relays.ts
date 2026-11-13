/** One relay per line, as the extension's relay box. */
export function relaysFromText(text: string): string[] {
  return text.split('\n').map((line) => line.trim()).filter((line) => line.length > 0);
}

export function relaysToText(relays: readonly string[]): string {
  return relays.join('\n');
}

/** Same as the extension's `formatVaultSize`. */
export function formatVaultSize(chars: number): string {
  if (chars < 1000) {
    return `${chars} B`;
  }
  if (chars < 10_000) {
    return `${(chars / 1000).toFixed(1)} KB`;
  }
  if (chars < 1_000_000) {
    return `${Math.round(chars / 1000)} KB`;
  }
  const mb = chars / 1_000_000;
  return `${mb >= 10 ? Math.round(mb) : mb.toFixed(1)} MB`;
}

export const SYNC_CONTENT_LIMIT = 1_000_000;
