// A browser with its own key, with the extension's own code (docs/DESIGN-browser-keys.md).
// For crates/gooti-core/tests/e2e_extension.rs.
//   browserkey <file.json>  with { browserKey, signer, secret, relay, name, step: "connect" | "sign" | "check" }
// secret is the one the pointer in the backup carries ("" for a stranger without it).
// connect: asks Gooti Desktop to allow the key (waits for Allow), then signs a note with it.
// sign: only signs, and prints the error the desktop answers with.
// check: the ping the popup sends on opening (checkGootiDesktop), and the error it gets.
import { readFileSync } from 'node:fs';
import { getPublicKey, verifyEvent } from 'nostr-tools';
import { SimplePool } from 'nostr-tools/pool';
import { browserCode } from '@ext/backup/device';
import { hexToBytes } from '@ext/backup/state';
import { bunkerSignEvent, checkGootiDesktop, connectGootiDesktop } from '@ext/nip46/client';

async function main() {
  const input = JSON.parse(readFileSync(process.argv[2], 'utf8')) as {
    browserKey: string;
    signer: string;
    secret: string;
    relay: string;
    name: string;
    step: 'connect' | 'sign' | 'check';
  };
  const bunker = { pubkey: input.signer, relays: [input.relay], secret: input.secret || null };
  const code = browserCode(getPublicKey(hexToBytes(input.browserKey)));
  if (input.step === 'check') {
    try {
      await checkGootiDesktop(input.browserKey, bunker, { pool: new SimplePool() });
      console.log(JSON.stringify({ code, checked: true }));
    } catch (error) {
      console.log(JSON.stringify({ code, error: error instanceof Error ? error.message : `not an Error: ${String(error)}` }));
    }
    process.exit(0);
  }
  if (input.step === 'connect') {
    try {
      await connectGootiDesktop(input.browserKey, bunker, input.name, { pool: new SimplePool(), timeoutMs: 20_000 });
    } catch (error) {
      console.log(JSON.stringify({ code, error: error instanceof Error ? error.message : `not an Error: ${String(error)}` }));
      process.exit(0);
    }
  }
  try {
    const signed = await bunkerSignEvent(
      input.browserKey,
      bunker,
      { kind: 1, content: 'signed with my own key', tags: [], created_at: 1_700_000_000 },
      { pool: new SimplePool(), timeoutMs: 15_000, desktop: { site: 'primal.net' } },
    );
    console.log(JSON.stringify({ code, signedBy: signed.pubkey, valid: verifyEvent(signed) }));
  } catch (error) {
    // The extension compares messages of Error objects: anything else would go unnoticed there.
    console.log(JSON.stringify({ code, error: error instanceof Error ? error.message : `not an Error: ${String(error)}` }));
  }
  process.exit(0);
}

void main().catch((error) => {
  console.error(error);
  process.exit(1);
});
