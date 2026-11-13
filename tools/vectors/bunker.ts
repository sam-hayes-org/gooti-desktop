// The extension's real NIP-46 client against the desktop bunker, for
// crates/gooti-core/tests/e2e_extension.rs. Prints what it got as JSON.
//   bunker <bunker:// link>
import { verifyEvent } from 'nostr-tools';
import { SimplePool } from 'nostr-tools/pool';
import {
  bunkerNip44Decrypt,
  bunkerNip44Encrypt,
  bunkerSignEvent,
  bunkerSwitchRelays,
  connectBunker,
} from '@ext/nip46/client';

async function main() {
  const link = process.argv[2];
  const pool = new SimplePool();
  const options = { pool, timeoutMs: 15_000 };
  const connected = await connectBunker(link, options);
  const { clientSecretKeyHex, bunker } = connected;
  const signed = await bunkerSignEvent(
    clientSecretKeyHex,
    bunker,
    { kind: 1, content: 'signed by the desktop', tags: [], created_at: 1_700_000_000 },
    options,
  );
  const ciphertext = await bunkerNip44Encrypt(clientSecretKeyHex, bunker, connected.pubkey, 'round trip', options);
  const plaintext = await bunkerNip44Decrypt(clientSecretKeyHex, bunker, connected.pubkey, ciphertext, options);
  const relays = await bunkerSwitchRelays(clientSecretKeyHex, bunker, options);
  console.log(
    JSON.stringify({
      pubkey: connected.pubkey,
      gootiDesktop: connected.gootiDesktop === true,
      signedBy: signed.pubkey,
      valid: verifyEvent(signed),
      content: signed.content,
      plaintext,
      relays,
    }),
  );
  process.exit(0);
}

void main().catch((error) => {
  console.error(error);
  process.exit(1);
});
