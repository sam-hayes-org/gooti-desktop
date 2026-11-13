// A fresh browser takes the desktop's vault and signs through it, with the extension's own
// code. For crates/gooti-core/tests/e2e_extension.rs.
//   custody <file.json>  with { payload, encryptionKeyHex, relay, privkey }
// Also checks the 2.1 custody rule: a browser's newer copy of the key loses to the move.
import { readFileSync } from 'node:fs';
import { verifyEvent } from 'nostr-tools';
import { SimplePool } from 'nostr-tools/pool';
import { openSharedDocument } from '@ext/backup/blob';
import { mergeSharedDocuments } from '@ext/backup/merge';
import { applySharedDocument, hexToBytes } from '@ext/backup/state';
import { bunkerSignEvent } from '@ext/nip46/client';

async function main() {
  const input = JSON.parse(readFileSync(process.argv[2], 'utf8')) as {
    payload: string;
    encryptionKeyHex: string;
    relay: string;
    privkey: string;
  };
  const document = await openSharedDocument(input.payload, hexToBytes(input.encryptionKeyHex));
  if (!document) {
    throw new Error('the extension read the vault as an old format');
  }
  let n = 0;
  const local = applySharedDocument(
    { identities: [], permissions: [], sites: [], selectedIdentityId: null },
    document,
    () => `id-${++n}`,
  );
  const identity = local.identities[0];
  const bunker = identity.bunker;
  if (!bunker?.clientSecretKey) {
    throw new Error('no shared session key');
  }
  // The pointer names the desktop's wss:// relays; this test relay runs on localhost.
  // As the background does for a row with custodyAt: ping, then name the site.
  const row = document.identities[0];
  const signed = await bunkerSignEvent(
    bunker.clientSecretKey,
    { ...bunker, relays: [input.relay] },
    { kind: 1, content: 'signed through the desktop', tags: [], created_at: 1_700_000_000 },
    {
      pool: new SimplePool(),
      timeoutMs: 15_000,
      ...(row.custodyAt !== undefined ? { desktop: { site: 'primal.net' } } : {}),
    },
  );
  // A browser that missed the move edits its copy of the key a minute later.
  const stale = {
    ...document,
    identities: [
      {
        pubkey: row.pubkey,
        privkey: input.privkey,
        nick: 'Edited with the key',
        createdAt: row.createdAt,
        updatedAt: row.updatedAt + 60_000,
      },
    ],
  };
  const merged = mergeSharedDocuments(stale, document);
  console.log(
    JSON.stringify({
      nick: identity.nick,
      hasPrivkey: identity.privkey !== undefined,
      askSite: bunker.askSite ?? null,
      signedBy: signed.pubkey,
      valid: verifyEvent(signed),
      custodyAt: row.custodyAt ?? null,
      staleKeyLoses: merged.identities[0].privkey === undefined,
    }),
  );
  process.exit(0);
}

void main().catch((error) => {
  console.error(error);
  process.exit(1);
});
