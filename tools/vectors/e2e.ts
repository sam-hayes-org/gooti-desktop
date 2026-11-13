// The extension's real backup code on a real relay, for crates/gooti-core/tests/e2e_extension.rs.
//   e2e start <relay> <phrase>  — a browser vault with "From the extension" backups to the relay
//   e2e check <relay> <phrase>  — a fresh browser joins and prints the nicks it got, as JSON
import { getPublicKey } from 'nostr-tools';
import { backUpShared } from '@ext/backup/manual';
import { activeSharedDocument } from '@ext/backup/merge';
import { deriveSharedKeys } from '@ext/backup/phrase';
import { defaultBackupRelayIO } from '@ext/backup/pool';
import { SharedDocument } from '@ext/backup/document';

const [mode, relay, phrase] = process.argv.slice(2);

async function main() {
  const keys = await deriveSharedKeys(phrase);
  const io = defaultBackupRelayIO();
  if (mode === 'start') {
    const privkey = '12'.repeat(32);
    const local: SharedDocument = {
      v: 2,
      identities: [
        {
          pubkey: getPublicKey(Buffer.from(privkey, 'hex')),
          privkey,
          nick: 'From the extension',
          color: '#336699',
          createdAt: '2026-11-11T08:00:00.000Z',
          updatedAt: Date.now(),
        },
      ],
      permissions: [],
      relays: [],
      sites: [],
    };
    const result = await backUpShared(local, keys, [relay], io);
    console.log(JSON.stringify({ published: result.published }));
  } else {
    const empty: SharedDocument = { v: 2, identities: [], permissions: [], relays: [], sites: [] };
    const result = await backUpShared(empty, keys, [relay], io);
    const nicks = activeSharedDocument(result.document).identities.map((identity) => identity.nick);
    console.log(JSON.stringify({ nicks, took: result.report.took }));
  }
  process.exit(0);
}

void main().catch((error) => {
  console.error(error);
  process.exit(1);
});
