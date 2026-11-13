// Copied from gooti-extension/projects/common/src/lib/identity/appearance.ts so an identity
// paints the same avatar and color in the extension and on the desktop. Keep them in step.

const COLOR = /^#[0-9a-f]{6}$/;

export interface AvatarCell {
  x: number;
  y: number;
}

/** A stored color, or undefined when the value is missing or not `#rrggbb`. */
export function readIdentityColor(stored: unknown): string | undefined {
  if (typeof stored !== 'string') {
    return undefined;
  }
  const color = stored.trim().toLowerCase();
  return COLOR.test(color) ? color : undefined;
}

/** Color for an identity addressed by its public key. A stored `#rrggbb` wins. */
export function colorForPubkey(pubkey: string, stored?: string | null): string {
  return readIdentityColor(stored) ?? colorFromPubkey(pubkey);
}

export function colorFromPubkey(pubkeyHex: string): string {
  const hex = pubkeyHex.toLowerCase().replace(/[^0-9a-f]/g, '').padEnd(2, '0');
  const hue = Math.round((Number.parseInt(hex.slice(0, 2), 16) / 255) * 359);
  return hslToHex(hue, 64, 58);
}

/** Symmetric 5×5 identicon. The same pubkey always paints the same cells. */
export function avatarCells(pubkeyHex: string): AvatarCell[] {
  const hex = pubkeyHex.toLowerCase().replace(/[^0-9a-f]/g, '').padEnd(10, '0');
  const cells: AvatarCell[] = [];
  for (let row = 0; row < 5; row++) {
    const bits = Number.parseInt(hex.slice(row * 2, row * 2 + 2), 16);
    const pattern = [bits & 1, bits & 2, bits & 4, bits & 2, bits & 1];
    pattern.forEach((on, x) => {
      if (on) {
        cells.push({ x, y: row });
      }
    });
  }
  if (cells.length === 0) {
    cells.push({ x: 2, y: 2 });
  }
  return cells;
}

function hslToHex(hue: number, saturation: number, lightness: number): string {
  const sat = saturation / 100;
  const light = lightness / 100;
  const chroma = (1 - Math.abs(2 * light - 1)) * sat;
  const hp = hue / 60;
  const x = chroma * (1 - Math.abs((hp % 2) - 1));
  let red = 0;
  let green = 0;
  let blue = 0;
  if (hp < 1) {
    red = chroma;
    green = x;
  } else if (hp < 2) {
    red = x;
    green = chroma;
  } else if (hp < 3) {
    green = chroma;
    blue = x;
  } else if (hp < 4) {
    green = x;
    blue = chroma;
  } else if (hp < 5) {
    red = x;
    blue = chroma;
  } else {
    red = chroma;
    blue = x;
  }
  const match = light - chroma / 2;
  const channel = (value: number) =>
    Math.round((value + match) * 255)
      .toString(16)
      .padStart(2, '0');
  return `#${channel(red)}${channel(green)}${channel(blue)}`;
}

/** `npub1abcd…wxyz` for lists. */
export function shortNpub(npub: string): string {
  return npub.length > 20 ? `${npub.slice(0, 10)}…${npub.slice(-6)}` : npub;
}
