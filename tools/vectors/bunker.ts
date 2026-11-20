// The extension's real NIP-46 client against the desktop bunker, for
// crates/gooti-core/tests/e2e_extension.rs. Prints what it got as JSON.
//   bunker <bunker:// link> <ws:// test relay>
// The extension accepts only wss:// relays from a link, and the test relay on localhost is plain
// ws://. So the link names wss://relay.invalid, and this pool connects there to the test relay.
import { verifyEvent } from 'nostr-tools';
import { SimplePool } from 'nostr-tools/pool';
import {
  bunkerNip44Decrypt,
  bunkerNip44Encrypt,
  bunkerSignEvent,
  bunkerSwitchRelays,
  connectBunker,
} from '@ext/nip46/client';

class TestRelayPool extends SimplePool {
  constructor(private readonly local: string) {
    super();
  }

  override ensureRelay(url: string, params?: Parameters<SimplePool['ensureRelay']>[1]) {
    return super.ensureRelay(url.includes('relay.invalid') ? this.local : url, params);
  }
}

async function main() {
  const link = process.argv[2];
  const pool = new TestRelayPool(process.argv[3]);
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
