# Gooti Desktop: plan for version 1

A desktop vault for Linux (first), later Mac and Windows. It:

- joins the extension's backup with the 12-word passphrase,
- works as a NIP-46 bunker for any Nostr app,
- offers per-identity custody, so browsers can sign through the desktop without anyone pasting a link.

Stack: Tauri 2 with a Rust core (`crates/gooti-core`) and an Angular 22 UI (`ui/`) that looks like the extension.
The wide window has a sidebar, because more features will follow.

## Open issues

Known problems that are not fixed yet. Remove an entry when it is fixed.

None right now.

## Decisions

| Topic | Decision |
|---|---|
| Stack | Tauri 2, Rust core, Angular UI. The server app later reuses `gooti-core`. |
| Key model | Custody per identity: **Synced** (as today) or **Desktop only** (the shared record carries a bunker pointer; the key is in the desktop record, which browsers cannot open). New desktop identities default to Desktop only. |
| Requests from your own browsers | No option. Since 2026-11-11 the extension sends every sign, encrypt, and decrypt from a bunker identity straight to the bunker: linking a site to the identity already answered the site question, and a second browser popup did not fit Primal's 8-second timeout. For other bunkers the browser still asks before each deletion (kind 5) or erase (kind 62); for Desktop only identities it does not, since the desktop sees the site and asks for these sensitive kinds itself (Basic and Ask every time). The old **Sites and the bunker** setting is gone from both apps; `bunker.askSite` stays in the sync format and is passed on unchanged for older versions. The desktop treats the browsers as one connection, "Gooti browsers", with the normal trust levels. Default: **Basic** (sensitive kinds ask on the desktop). |
| Layout | Wide window (about 1100 × 720, minimum 800 × 560) with a sidebar: Identities, Connections, Activity, Settings. Approval and connect windows stay small. |
| Platforms | Linux first: AppImage, deb, rpm, AUR. Windows second: an NSIS installer per user, built, tested and released by CI (no Windows machine here; tests also run as Windows binaries under Wine). CI compiles Mac only to catch errors. |
| Signing | Unsigned alpha. |
| Relays | Bunker default `wss://relay.getgooti.com` only (relay.nsec.app was dropped: it often timed out). Users can add more. relay.getgooti.com must accept kind 24133 (ephemeral, filtered by `#p`), and optionally NIP-42 AUTH. |
| License | Public domain (Unlicense), like the extension. |
| Branches | Work happens on `alpha`. `main` holds releases. |
| Naming (2026-11-12) | In both apps, what the passphrase opens on the relays is the **backup** ("Back up now", "Restore from backup"), never "sync". The local store has no name: "Unlock Gooti", "Gooti password". Custody is "Where the key is used": Browsers and desktops, or Desktop only. Users see one backup; "shared record" and "desktop record" below are names for the docs only. Rules in `CLAUDE.md`. |

## Design: the desktop record

Status: **built (2026-11-12).** Code: `crates/gooti-core/src/backup/desktop.rs` (format, merge) and
`crates/gooti-core/src/vault/desktop.rs` (what goes in, what comes back).

### The problem it solves

The passphrase opens the **shared record** (kind 30078, `d` = `gooti-shared`), which every browser and
desktop reads and writes. What lived only on the desktop had no backup: the private keys of Desktop only
identities, the bunker signer keys (every pointer and `bunker://` link names one), and the bunker
connections with their trust and rules. A reset lost them, and a restore brought back pointers nothing
could answer. The shared record cannot hold them: browsers read it, and "Desktop only" means browsers
never get the key.

### The desktop record

A second record, opened by the same passphrase with its own keys. Only Gooti Desktops read and write it,
and all desktops on one passphrase share it. Neither the extension nor the desktop keeps the 12 words,
only the keys derived from them, so a browser cannot open or replace it. The extension does not change.

- **Keys**: HKDF-SHA256 over the BIP-39 seed, salt `gooti-backup-v1`, info `gooti-desktop-encrypt-v1` and
  `gooti-desktop-sign-v1` (with the same `:1` to `:7` retry for the signing key).
- **Blob**: as the shared record (`0x01` ‖ IV ‖ AES-256-GCM, base64, at most 1 MB). The JSON carries
  `"gootiDesktop": 1`, so neither record opens as the other.
- **Event**: kind 30078 with `d` = `gooti-desktop`, signed by the desktop signing key, on the backup
  relays. relay.getgooti.com accepts `gooti-shared` and `gooti-desktop` only (`../gooti-relay`).
- **Rows** (each with the time of its last change; a deleted row stays as a tombstone; a merge keeps the
  later row, a tie keeps the local one):
  - `keys`: identity pubkey → private key, for **every** key a desktop holds, not only Desktop only ones.
    Then a move to the desktop needs no new row, only the order below.
  - `signers`: signer pubkey → identity, secret, and `servedBy` (the desktop that answers on it). An
    identity can have several: each desktop that starts serving one makes its own.
  - `connections`: connection id → the connection without its last-used time. When two desktops made a
    connection for the same app and identity, the smaller id stays everywhere.
  - `bunkerRelays`: stamped with the time the user set them. Defaults never override another desktop's.
- **Not in it**: window, lock, autostart and update settings; pause; open invites; Activity; last-used times.
- **Deletions** only for a reason this desktop knows: a connection it revoked or an app that logged out,
  or an identity the shared record deleted. An identity whose shared row has not arrived yet keeps its
  rows, so a backup that fails halfway never deletes a key.

### Flows

- **Every backup sends the desktop record first, then the shared record.** When the desktop record cannot
  be sent, the shared record is not sent either. A key moved to the desktop therefore stays in the shared
  record (browsers keep it) until the desktop record has it. The note planned "the key stays Synced" in
  that case; this order gives the same safety without a special case.
- **Restore from backup** reads both records before storing anything. A pointer whose signer key and key
  are in the desktop record becomes a Desktop only identity with its key again, with the same signer key,
  so the browsers' pointer and other apps' links keep working.
- **Use an existing backup** (join) merges the desktop record right after the shared one.
- **Custody follows the later move.** When another desktop moved a key later (to itself, or back to
  browsers), this desktop follows. A desktop never puts a Desktop only key back into the shared record.
  This fixed the earlier open issue "two Gooti desktops can bring a moved key back into the shared record".
- **Reset** shows its warning only while the backup is behind (`SyncInfo.desktopCurrent` false) or off.
  Its action is "Back up now"; the keys stay Desktop only.

### Two desktops on one passphrase

Each signer key has one `servedBy` desktop (a random id made with the vault). Only that desktop listens
with it, so one desktop answers. A restored desktop has a new id, so it starts by answering nothing:
the Identities page names the identities "another Gooti Desktop" signs for, with **Sign here**, and each
identity page has **Sign here instead**. The desktop that answered before stops at its next backup.
Changes from the note: no desktop name is shown (the core has none), and the question after a restore
is that card on the Identities page, not a step on the restore screen.

### What the user sees

- "Desktop only": browsers never get the key; the backup keeps it for Gooti Desktops. Anyone with the
  12 words can take every identity, Desktop only ones too.
- **Key missing** (was "Other desktop"): a Gooti Desktop pointer whose key is in no desktop record. Only
  then is the key really lost.
- The browser sees the words once, when it sets up or joins. A browser compromised at that moment could
  derive the desktop keys too. Accepted: it could take the synced keys at the same moment.

### Tests

`backup_flows.rs`: restore after a reset (keys, signer keys, apps, serving), a missing desktop record,
the send order with the desktop record refused, two desktops (serving, taking over, a revoked app),
custody that follows the other desktop. `e2e_extension.rs`: the extension's own code signs through its
old pointer after the desktop was reset and restored. `backup/desktop.rs`: the shared keys open nothing.

## Compatibility with the extension

The Rust backup code must match `gooti-extension/projects/common/src/lib/backup` byte for byte:

- **Keys from the passphrase** (`phrase.ts`): the BIP-39 seed goes through HKDF-SHA256 with salt `gooti-backup-v1`.
  The info labels are `gooti-shared-encrypt-v1` (AES key) and `gooti-shared-sign-v1` (signing key). If the signing key
  isn't a valid secp256k1 scalar, retry with `:1` up to `:7`. (Until 2026-11-12: `gooti-sync-v1`, `gooti-sync-*-v1`.)
- **Blob** (`blob.ts`): `0x01` ‖ 12-byte IV ‖ AES-256-GCM ciphertext, base64. Over 1 MB is rejected.
- **Event** (`event.ts`): kind 30078 with `["d","gooti-shared"]` (until 2026-11-12: `gooti-vault`).
- **Document** (`document.ts`): v2. Each identity has exactly one of `privkey` or `bunker`.
- **Merge** (`merge.ts`, `state.ts`): the higher revision wins, a tie keeps local, and a synced conversation key fills a local row without one.
  A winning bunker row replaces a local key (`localIdentityFrom`).
- **Bunker client** (`nip46/client.ts`, nostr-tools `BunkerSigner`): the desktop must answer `connect` (with a secret),
  `get_public_key`, `sign_event`, NIP-04 and NIP-44 encrypt and decrypt, and `switch_relays`.

Tests run both ways: `tools/vectors` uses the extension's own TS to write cases the Rust tests must open, and the reverse.
An end-to-end test runs the extension's `connectBunker` against the desktop bunker on a local relay.

## Milestones

| # | Milestone | Done when |
|---|---|---|
| M0 | Scaffold: workspace, Tauri with Angular, extension styles and Inter, sidebar shell, tray, CI, `CLAUDE.md` | **Done.** The Gooti window opens on Linux |
| M1 | Vault: password (Argon2id, at least 4 characters like the extension), change password, optional "Unlock when Gooti starts" via the OS keychain, lock after idle (off by default), lock on screen lock and sleep (on by default). Identities: generate a key (random, like the extension's "Generate private key"; no NIP-06, so the 12 words stay the sync passphrase), import nsec/hex/ncryptsec, show nsec, export ncryptsec, nick, color, delete | **Done.** Tests pass; writes are atomic |
| M2 | Sync: Rust implementation, test cases, join with passphrase, restore, Sync now, automatic sync, change panel | **Done.** Checked against the extension's own code both ways, and end to end through a relay |
| M3 | Bunker: `bunker://` per identity (single use, 1 h expiry, QR, `gooti=1` so the extension knows the desktop holds the key), paste `nostrconnect://`, all NIP-46 methods incl. `logout`, relay pool with NIP-42, connections, revoke, pause | **Done.** The extension's own NIP-46 client connects, signs and encrypts through it (e2e test) |
| M4 | Permissions and approvals: Ask (default), Basic, Trust per app; remembered Approve or Reject per kind or method for 5 min, 1 h, 1 day or always; a remembered Reject wins; sensitive kinds remembered for 1 h at most; events dated more than 10 min ahead flagged; approval window; tray (Open, Lock now, Pause all apps, Quit); notifications; activity | **Done.** Rules tested over a real relay; UI tested |
| M5 | Desktop only: offered by default for new desktop identities, Move key to desktop, Make synced again, a pre-approved pointer with `askSite`, the "Sites and the bunker" setting, re-moving a key that returns in a merge | **Done.** The extension's own code applies the pointer and signs through the desktop without a link (e2e) |
| M6 | Release (Linux): AppImage, deb, rpm, AUR, updater, keep running in the tray, start at login | **Done in the repo.** The updater was tested end to end: a 0.1.0 AppImage found a signed 0.1.1, installed it and restarted. Not yet done: the GitHub repository, its secrets, the first release, the AUR account, the website page (later) |
| M7 | Release (Windows): NSIS installer per user, `nostrconnect://` from the installer, updater runs the new installer, lock on screen lock (WTS session notification) and sleep (`PBT_APMSUSPEND`), left click on the tray opens the window | **In the repo, not yet run on Windows.** Clippy and every test pass as Windows binaries (cross-compiled, run under Wine, keychain included). Not yet done: a Develop run on GitHub, a person trying the installer on Windows, code signing |

**Extension 2.1**: **Done** in both repos (the extension's part waits for its owner to commit):

- A custody marker: `custodyAt` on a synced identity, the time its key last moved to a Gooti Desktop
  or back. When one row holds the key and the other is a bunker pointer, the later `custodyAt` wins
  (absent counts as 0); otherwise the newer row wins as before. A desktop that sees an older or missing
  custody time for a key it holds (a 2.0 browser dropped the field) sends its row again.
- The site in requests to the desktop: only for an identity whose synced row is a pointer with
  `custodyAt` and the same signer key. The extension adds `{"site": host}` after the standard NIP-46
  parameters. The desktop reads it only from the "Gooti browsers" connection and shows it in the
  approval window and Activity.
- When a bunker does not answer: a ping first (8 s, skipped if the desktop answered in the last
  2 minutes), then up to 5.5 minutes for the desktop's own 5-minute approval. A small notice window
  (at most once in 5 minutes) and the reason in Activity, for any bunker, worded per kind.

## Notes for M5

- A Desktop only identity syncs as a bunker pointer: this desktop's signer key for it, the bunker's
  `wss://` relays (the extension accepts only those), `askSite`, and a session key made here. All synced
  browsers share that session key as one connection, "Gooti browsers", trust Basic. It cannot be removed
  on its own; making the identity synced again ends it.
- Every edit is stamped later than the row it replaces, so an edit wins even over a device whose clock
  runs ahead. A Desktop only key that comes back in a merge is replaced by the pointer and sent again.
- Another Gooti desktop keeps a private key it holds, even when a pointer wins the merge, and follows the
  custody of the later move. Browsers drop the key.
- Extension 2.1 added the custody marker, the site in requests, and the "did not answer" notice.
- The keys of Desktop only identities, the signer keys and the connections are in the desktop record:
  see [Design: the desktop record](#design-the-desktop-record).

## Notes for M3 and M4

- Each identity has its own signer key for the bunker, so relays do not see which identity an app uses.
  The M5 bunker pointer names that signer key.
- Requests that ask open a small always-on-top approval window and a system notification. The
  Connections page lists them too, as a fallback.
- Basic approves encryption and decryption too; only sensitive kinds ask. Sensitive kinds: profile, follow
  list, deletion, erase account, mute list, relay list, DM relays, relay and HTTP sign-in, Blossom auth,
  wallet events.
- While the vault is locked the bunker does not listen: its keys are sealed in the vault.
- Default bunker relays: relay.getgooti.com only. It must accept kind 24133.
- Relays (2026-11-12): two lists of defaults. **Backup relays** (Backup page) and **Default relays for
  apps** (Settings, `bunker.relays`). The second is only a template: it fills a new bunker link (Connect
  new app), a new Desktop only identity, and "Move key to desktop", and each keeps its own copy, so
  changing the default changes nothing in use. An app from a `bunker://` link keeps the relays its link
  named; a `nostrconnect://` app keeps its own; Gooti browsers of a Desktop only identity keep theirs on
  their connection (editable on its page; the pointer names them and backup takes a change to the
  browsers). The desktop listens only on relays in use and on those of links still waiting
  ("Listening on" in Connections). Apps and browsers stored earlier without relays get the default once
  on unlock.
  Its setup and the change for this live in `../gooti-relay`. It refuses a filter with more than one `#p`
  value, so the bunker opens one subscription per signer key (tested against a strict test relay).

## Notes for M2

- A deleted identity stays in the local vault as a tombstone (`deletedAt`, no private key). The extension's
  shared record requires exactly one of `privkey` or `bunker`, even on a tombstone. Check how `state.ts`
  publishes a deleted identity before mapping tombstones to the shared record.

## Not in version 1

- Server app
- FROST
- NIP-70 socket and native messaging
- Social notifications and NIP-38
- Mobile and NIP-55
- Languages other than English
