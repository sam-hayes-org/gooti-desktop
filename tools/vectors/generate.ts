// Runs the Gooti extension's own backup code and writes what it produced, so the Rust port can be
// checked against it. Needs ../gooti-extension with its node_modules. Run: npm run vectors
import { writeFileSync } from 'node:fs';
import { getPublicKey } from 'nostr-tools';
import { hexToBytes } from 'nostr-tools/utils';
import { colorFromPubkey } from '@ext/identity/appearance';
import { openSharedDocument, sealSharedDocument, sharedContentChars } from '@ext/backup/blob';
import { describeChanges } from '@ext/backup/changes';
import { assertSharedDocument, SharedDocument, SharedIdentity } from '@ext/backup/document';
import { buildBackupEvent } from '@ext/backup/event';
import { activeSharedDocument, mergeSharedDocuments } from '@ext/backup/merge';
import { deriveSharedKeys, isPassphrase, normalizePassphrase } from '@ext/backup/phrase';
import { canonicalSharedDocument } from '@ext/backup/state';

const out = process.argv[2];
if (!out) {
  throw new Error('usage: generate <output.json>');
}

const hex = (bytes: Uint8Array) => Buffer.from(bytes).toString('hex');
const key = (n: number) => n.toString(16).padStart(2, '0').repeat(32);
const pub = (privkey: string) => getPublicKey(hexToBytes(privkey));

const ALICE = key(0x11);
const BOB = key(0x22);
const CAROL = key(0x33);
const PHONE = pub(key(0x44));

function local(privkey: string, nick: string, updatedAt: number, extra: Partial<SharedIdentity> = {}): SharedIdentity {
  return { pubkey: pub(privkey), privkey, nick, createdAt: '2026-01-02T03:04:05.678Z', updatedAt, ...extra };
}

function bunker(nick: string, updatedAt: number, extra: Partial<SharedIdentity['bunker']> = {}): SharedIdentity {
  return {
    pubkey: PHONE,
    nick,
    createdAt: '2026-02-03T04:05:06.789Z',
    updatedAt,
    color: '#3a7bd5',
    bunker: { pubkey: key(0x55).slice(0, 64), relays: ['wss://relay.example'], secret: null, ...extra },
  };
}

function doc(parts: Partial<SharedDocument>): SharedDocument {
  return { v: 2, identities: [], permissions: [], relays: [], sites: [], ...parts };
}

const full = doc({
  identities: [
    local(ALICE, 'Alice', 1_700_000_000_000, { color: '#a1b2c3' }),
    local(BOB, 'Bob', 1_700_000_000_500),
    bunker('Phone', 1_700_000_001_000, { askSite: false, clientSecretKey: key(0x66), secret: 's3cret' }),
    local(CAROL, 'Carol', 1_700_000_000_000, { deletedAt: 1_700_000_002_000 }),
  ],
  permissions: [
    { identityPubkey: pub(ALICE), host: 'primal.net', method: 'signEvent', methodPolicy: 'allow', kind: 1, updatedAt: 10 },
    { identityPubkey: pub(ALICE), host: 'primal.net', method: 'nip44.decrypt', methodPolicy: 'deny', updatedAt: 11 },
    { identityPubkey: pub(BOB), host: 'coracle.social', method: 'getPublicKey', methodPolicy: 'allow', updatedAt: 12, deletedAt: 13 },
  ],
  sites: [
    { host: 'primal.net', identityPubkey: pub(ALICE), updatedAt: 20 },
    { host: 'localhost:5173', identityPubkey: pub(BOB), updatedAt: 21, deletedAt: 22 },
  ],
});

const phrases = [
  'abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about',
  '  legal winner thank year wave sausage worth useful legal winner thank yellow \n',
  'letter advice cage absurd amount doctor acoustic avoid letter advice cage above',
];

async function main() {
  const keys = [];
  for (const phrase of phrases) {
    const derived = await deriveSharedKeys(phrase);
    keys.push({
      phrase,
      normalized: normalizePassphrase(phrase),
      signingKey: hex(derived.signingKey),
      encryptionKey: hex(derived.encryptionKey),
      pubkey: derived.pubkey,
    });
  }
  const invalidPhrases = [
    'abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon',
    'Abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about',
    'hello world',
  ].map((phrase) => ({ phrase, valid: isPassphrase(phrase) }));

  const main = await deriveSharedKeys(phrases[0]);
  const payload = await sealSharedDocument(full, main.encryptionKey);
  const event = buildBackupEvent(main.signingKey, payload, 1_762_000_000);
  const oldPayload = await sealOld(main.encryptionKey);
  if ((await openSharedDocument(oldPayload, main.encryptionKey)) !== null) {
    throw new Error('expected an old document to open as null');
  }

  const merges = mergeCases().map(({ name, local, remote }) => {
    const merged = remote ? mergeSharedDocuments(local, remote) : local;
    const published = remote === null || canonicalSharedDocument(merged) !== canonicalSharedDocument(remote);
    const changed = canonicalSharedDocument(merged) !== canonicalSharedDocument(local);
    return {
      name,
      local,
      remote,
      merged,
      active: activeSharedDocument(merged),
      published,
      changed,
      report: describeChanges(local, remote, merged, published),
    };
  });

  const documents = validityCases().map(({ name, value }) => {
    try {
      const copy = structuredClone(value);
      assertSharedDocument(copy);
      return { name, value, valid: true, normalized: copy };
    } catch {
      return { name, value, valid: false };
    }
  });

  const colors = [pub(ALICE), pub(BOB), pub(CAROL), PHONE, '00'.repeat(32), 'ff'.repeat(32)].map((pubkey) => ({
    pubkey,
    color: colorFromPubkey(pubkey),
  }));

  writeFileSync(
    out,
    JSON.stringify(
      {
        source: 'gooti-extension projects/common/src/lib/backup, generated by tools/vectors/generate.ts',
        keys,
        invalidPhrases,
        blob: {
          phrase: phrases[0],
          document: full,
          payload,
          oldPayload,
          contentChars: sharedContentChars(full),
          event,
        },
        merges,
        documents,
        colors,
      },
      null,
      2,
    ) + '\n',
  );
}

/** A v1 document sealed the same way, as an older extension wrote it. */
async function sealOld(encryptionKey: Uint8Array): Promise<string> {
  const iv = crypto.getRandomValues(new Uint8Array(12));
  const aes = await crypto.subtle.importKey('raw', new Uint8Array(encryptionKey), 'AES-GCM', false, ['encrypt']);
  const plaintext = new TextEncoder().encode(JSON.stringify({ v: 1, identities: [] }));
  const ciphertext = new Uint8Array(await crypto.subtle.encrypt({ name: 'AES-GCM', iv }, aes, plaintext));
  return Buffer.from(new Uint8Array([1, ...iv, ...ciphertext])).toString('base64');
}

function mergeCases(): { name: string; local: SharedDocument; remote: SharedDocument | null }[] {
  const a = local(ALICE, 'Alice', 100);
  return [
    { name: 'no remote vault yet', local: doc({ identities: [a] }), remote: null },
    { name: 'same document', local: doc({ identities: [a] }), remote: doc({ identities: [a] }) },
    { name: 'remote only identity is added', local: doc({}), remote: doc({ identities: [a] }) },
    { name: 'local only identity is kept and sent', local: doc({ identities: [a] }), remote: doc({}) },
    {
      name: 'newer remote nick wins',
      local: doc({ identities: [a] }),
      remote: doc({ identities: [{ ...a, nick: 'Alicia', updatedAt: 200 }] }),
    },
    {
      name: 'newer local nick wins',
      local: doc({ identities: [{ ...a, nick: 'Alicia', updatedAt: 200 }] }),
      remote: doc({ identities: [a] }),
    },
    {
      name: 'tie keeps local',
      local: doc({ identities: [{ ...a, nick: 'Local' }] }),
      remote: doc({ identities: [{ ...a, nick: 'Remote' }] }),
    },
    {
      name: 'newer remote delete wins and keeps the tombstone',
      local: doc({ identities: [a] }),
      remote: doc({ identities: [{ ...a, deletedAt: 300 }] }),
    },
    {
      name: 'local edit after remote delete brings it back',
      local: doc({ identities: [{ ...a, updatedAt: 400 }] }),
      remote: doc({ identities: [{ ...a, deletedAt: 300 }] }),
    },
    {
      name: 'tie fills a missing conversation key',
      local: doc({ identities: [bunker('Phone', 500)] }),
      remote: doc({ identities: [bunker('Phone remote', 500, { clientSecretKey: key(0x77) })] }),
    },
    {
      name: 'older remote does not give its conversation key',
      local: doc({ identities: [bunker('Phone', 600)] }),
      remote: doc({ identities: [bunker('Phone', 500, { clientSecretKey: key(0x77) })] }),
    },
    {
      name: 'tie with a different bunker secret keeps local without a key',
      local: doc({ identities: [bunker('Phone', 500)] }),
      remote: doc({ identities: [bunker('Phone', 500, { secret: 'other', clientSecretKey: key(0x77) })] }),
    },
    {
      name: 'permissions and sites merge by key and revision',
      local: doc({
        identities: [a],
        permissions: [
          { identityPubkey: a.pubkey, host: 'primal.net', method: 'signEvent', methodPolicy: 'allow', kind: 1, updatedAt: 10 },
          { identityPubkey: a.pubkey, host: 'primal.net', method: 'signEvent', methodPolicy: 'allow', updatedAt: 10 },
        ],
        sites: [{ host: 'primal.net', identityPubkey: a.pubkey, updatedAt: 10 }],
      }),
      remote: doc({
        identities: [a],
        permissions: [
          { identityPubkey: a.pubkey, host: 'primal.net', method: 'signEvent', methodPolicy: 'deny', kind: 1, updatedAt: 20 },
          { identityPubkey: a.pubkey, host: 'snort.social', method: 'nip04.encrypt', methodPolicy: 'allow', updatedAt: 5 },
        ],
        sites: [
          { host: 'primal.net', identityPubkey: pub(BOB), updatedAt: 5 },
          { host: 'snort.social', identityPubkey: a.pubkey, updatedAt: 5, deletedAt: 6 },
        ],
      }),
    },
    ...custodyCases(a),
    { name: 'full document against empty', local: full, remote: doc({}) },
    { name: 'empty against full document', local: doc({}), remote: full },
  ];
}

/** Extension 2.1: a key against a bunker pointer is decided by custodyAt first. */
function custodyCases(a: SharedIdentity): { name: string; local: SharedDocument; remote: SharedDocument | null }[] {
  const pointer = (updatedAt: number, custodyAt?: number): SharedIdentity => ({
    ...a,
    privkey: undefined,
    nick: 'On the desktop',
    updatedAt,
    bunker: bunker('x', 1).bunker,
    ...(custodyAt === undefined ? {} : { custodyAt }),
  });
  const withKey = (updatedAt: number, custodyAt?: number): SharedIdentity => ({
    ...a,
    nick: 'With the key',
    updatedAt,
    ...(custodyAt === undefined ? {} : { custodyAt }),
  });
  return [
    {
      name: 'custody: a newer key row does not undo a move',
      local: doc({ identities: [withKey(900)] }),
      remote: doc({ identities: [pointer(700, 700)] }),
    },
    {
      name: 'custody: a move wins as the local row too',
      local: doc({ identities: [pointer(700, 700)] }),
      remote: doc({ identities: [withKey(900)] }),
    },
    {
      name: 'custody: a later move back wins over a newer pointer',
      local: doc({ identities: [pointer(990, 700)] }),
      remote: doc({ identities: [withKey(800, 800)] }),
    },
    {
      name: 'custody: the same custody time falls back to the newer row',
      local: doc({ identities: [pointer(700, 700)] }),
      remote: doc({ identities: [withKey(900, 700)] }),
    },
    {
      name: 'custody: two pointers use the newer row',
      local: doc({ identities: [pointer(700, 700)] }),
      remote: doc({ identities: [pointer(900)] }),
    },
  ];
}

function validityCases(): { name: string; value: unknown }[] {
  const a = local(ALICE, 'Alice', 1);
  return [
    { name: 'full document', value: full },
    { name: 'sites missing', value: { v: 2, identities: [a], permissions: [], relays: [] } },
    { name: 'relays missing and getRelays dropped', value: { v: 2, identities: [a], permissions: [{ identityPubkey: a.pubkey, host: 'x.com', method: 'getRelays', methodPolicy: 'allow', updatedAt: 1 }] } },
    { name: 'old relay rows dropped', value: { v: 2, identities: [], permissions: [], relays: [{ identityPubkey: a.pubkey, url: 'wss://r', read: true, write: true, updatedAt: 1 }] } },
    { name: 'wrong version', value: { v: 3, identities: [], permissions: [] } },
    { name: 'privkey does not match pubkey', value: doc({ identities: [{ ...a, privkey: BOB }] }) },
    { name: 'privkey and bunker together', value: doc({ identities: [{ ...a, bunker: bunker('x', 1).bunker }] }) },
    { name: 'neither privkey nor bunker', value: doc({ identities: [{ ...a, privkey: undefined }] }) },
    { name: 'bunker without relays', value: doc({ identities: [bunker('x', 1, { relays: [] })] }) },
    { name: 'bunker relay not wss', value: doc({ identities: [bunker('x', 1, { relays: ['ws://plain'] })] }) },
    { name: 'bunker secret missing', value: doc({ identities: [{ ...bunker('x', 1), bunker: { pubkey: PHONE, relays: ['wss://r'] } as never }] }) },
    { name: 'bad color', value: doc({ identities: [{ ...a, color: 'red' }] }) },
    { name: 'upper case color is accepted', value: doc({ identities: [{ ...a, color: '#AABBCC' }] }) },
    { name: 'unknown method', value: doc({ permissions: [{ identityPubkey: a.pubkey, host: 'x.com', method: 'nip99.do' as never, methodPolicy: 'allow', updatedAt: 1 }] }) },
    { name: 'site host with a slash', value: doc({ sites: [{ host: 'x.com/a', identityPubkey: a.pubkey, updatedAt: 1 }] }) },
    { name: 'custody time', value: doc({ identities: [{ ...a, custodyAt: 5 }] }) },
    { name: 'custody time that is not a number', value: doc({ identities: [{ ...a, custodyAt: 'soon' as never }] }) },
    { name: 'fractional timestamps', value: doc({ identities: [{ ...a, updatedAt: 1.5 }] }) },
    { name: 'unknown fields are kept', value: { ...doc({ identities: [{ ...a, future: 1 } as never] }), future: true } },
  ];
}

void main();
