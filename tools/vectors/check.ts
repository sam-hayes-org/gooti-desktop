// The other direction: opens what the Rust code sealed with the extension's own code.
// Run: npm run vectors:check (after `cargo test` wrote target/vectors/rust.json).
import { readFileSync } from 'node:fs';
import { Event } from 'nostr-tools';
import { openSharedDocument } from '@ext/backup/blob';
import { assertSharedDocument } from '@ext/backup/document';
import { readBackupPayload } from '@ext/backup/event';
import { deriveSharedKeys } from '@ext/backup/phrase';
import { canonicalSharedDocument } from '@ext/backup/state';

interface RustOutput {
  phrase: string;
  pubkey: string;
  event: Event;
  document: unknown;
}

async function main() {
  const file = process.argv[2];
  if (!file) {
    throw new Error('usage: check <rust.json>');
  }
  const rust = JSON.parse(readFileSync(file, 'utf8')) as RustOutput;
  const keys = await deriveSharedKeys(rust.phrase);
  if (keys.pubkey !== rust.pubkey) {
    throw new Error(`pubkey differs: ${keys.pubkey} vs ${rust.pubkey}`);
  }
  if (rust.event.pubkey !== keys.pubkey) {
    throw new Error('event is not signed by the shared key');
  }
  const opened = await openSharedDocument(readBackupPayload(rust.event), keys.encryptionKey);
  if (!opened) {
    throw new Error('the extension read the Rust document as an old document');
  }
  const expected = structuredClone(rust.document);
  assertSharedDocument(expected);
  if (canonicalSharedDocument(opened) !== canonicalSharedDocument(expected)) {
    throw new Error('the opened document differs from what Rust sealed');
  }
  console.log(`OK: the extension opened the Rust vault event ${rust.event.id.slice(0, 12)}… with ${opened.identities.length} identities.`);
}

void main().catch((error) => {
  console.error(error);
  process.exit(1);
});
