# Design: a key per browser

Status: agreed 2026-11-16, built; shared key retired 2026-11-17. Touches Gooti Desktop and the extension.

## The problem

For each Desktop only identity, Gooti Desktop makes one session key that every browser shares, the
"Gooti browsers" connection. The key travels in the backup, inside the identity's bunker pointer
(`clientSecretKey`, `bunker/store.rs` `browser_pointer`). That makes setup one-click, but:

- A lost laptop keeps that key. Anyone who unlocks Gooti there can ask the desktop to sign as every
  Desktop only identity (with trust Basic, notes even without a question).
- It cannot be taken back. A new key would travel in the backup too, and the lost laptop can still
  open the backup: it has the passphrase.
- All browsers are one connection. The desktop cannot tell them apart or cut off one.

Desktop only keys are never on a browser. With this fixed, a lost laptop costs the identities whose
key was on it, but not the Desktop only ones.

## The idea

Each browser makes its own session key and keeps it to itself: it never goes into the backup. The
browser asks Gooti Desktop once to sign through it; you approve on the desktop. The desktop then
signs only for approved browser keys, lists each browser by name, and **Disconnect** cuts one off at
once. The others keep working: their keys never changed.

## How a browser connects

1. **A Desktop only identity reaches the browser** through the backup: the pointer (signer key and
   relays), without `clientSecretKey`.
2. **The browser asks by itself, but only while you use it.** Never in the background: only right
   after you enter the passphrase in that browser (set up from a backup, or "Use an existing
   backup"), or when you open the popup and a Desktop only identity there is not connected yet. So an
   approval window on the desktop always follows something you just did in front of that browser,
   and an unexpected one stays suspicious. Moving a key to the desktop stays one click: an approved
   browser needs nothing more, and each new browser costs one **Allow**, once. The name is this
   browser's name in Devices ("Firefox on Linux" until renamed there).
3. **The request.** The browser makes its key (once; one key for all Desktop only identities) and
   sends a NIP-46 `connect` to each such identity's signer key, with its name in the client metadata
   and the pointer's `secret` as the connect secret. This is the standard NIP-46 path the desktop
   already listens on; nothing new goes on the relays.
4. **The browser waits** and shows: "Approve on Gooti Desktop. Code K7F-2QX."
5. **Gooti Desktop asks once per browser** (a window, like a new app):
   *"Work laptop" wants to sign through this desktop — Firefox on Linux · code K7F-2QX · asked just
   now* — **Allow** or **Deny**. Allow covers every Desktop only identity, and ones made later: a
   `connect` from an approved browser key is accepted without asking.
6. **Done.** Requests carry the site as today, and the desktop's trust level applies per identity, the
   same for every browser (default Basic, set on the identity's page under **Browsers**). An unanswered request expires after 5 minutes, like every request
   to the desktop; the browser asks again at the next of the two moments.

A signer key is visible to anyone who reads the relay (NIP-46 requests name it in their `p` tag), so
the signer key alone must not be enough to ask. The pointer therefore carries a `secret`, derived
from the identity's private key (`bunker::state::browser_secret`, the same on every Gooti Desktop),
and a browser `connect` without it is refused without a question (added 2026-11-18). So only someone
who can open the backup, which means someone with the passphrase, can ask. The approval is the guard
against them. The window shows the name, the code and how long ago it was asked; you allow only a
request you just made yourself. **Deny** also blocks that key, so it cannot ask again. A block is
kept for 90 days, on every Gooti Desktop, then forgotten, so the list does not grow forever
(decided 2026-11-19).

## Telling browsers apart

Three things, so three "Firefox on Linux" are never a guess:

- **A name** chosen when the browser connects (step 2).
- **Dates** per browser: approved, last signature through the desktop, last backup (from Devices).
  The lost one is usually the one that went quiet.
- **A code** from the browser's key: SHA-256 of the 32-byte public key, first 30 bits in Crockford
  base32, as `XXX-XXX` (for example `K7F-2QX`). The browser shows it in Settings → Devices next to
  "This browser"; the desktop shows it next to each browser. You cannot check the lost laptop, but
  you can open the browsers you still have: the code you cannot find in your hands is the lost one.

## Disconnect

Gooti Desktop → Connections → **Browsers**: one row per browser with name, code, approved, last
used, and **Disconnect**. Disconnect removes that browser's connections for every
identity and blocks its key for 90 days. Its next request fails with "Gooti Desktop disconnected this browser."
The extension then forgets the allowance. It also learns it without a request: when its popup
opens, an allowed browser pings the desktop with its key (at most once a minute). The popup then
says "Gooti Desktop disconnected this browser" with **Connect again**, which makes a new key and asks
again, so a browser disconnected by mistake comes back with one **Allow**. It does not ask by itself
after a Disconnect (decided 2026-11-17). A lost laptop could press Connect again too, and you
**Deny** it.

With two Gooti Desktops on one passphrase, the approved and blocked browser keys travel in the
desktop record (which browsers cannot open), so Disconnect on one desktop holds on both.

## The shared key is gone

Decided 2026-11-17: no transition period. Users are still few, and the extension updates itself.

- **Gooti Desktop** no longer puts `clientSecretKey` in the pointers and no longer accepts the shared
  key. At unlock it removes the shared key's connections ("Gooti browsers") and remembers the key
  (`BunkerData::retired_shared_keys`): its requests get "Update Gooti in this browser to sign through
  Gooti Desktop." The identity's browser relays, which that connection held, are kept per identity
  (`BunkerData::browser_relays`, also in the desktop record).
- **Extension (new)** uses its own key once allowed. Before that it still tries the key it stored
  (a pasted `bunker://` link has a key of its own that must keep working); for the retired shared key
  the desktop refuses, and the extension shows "Allow this browser in Gooti Desktop first". The popup
  asks for Allow when it opens.
- **Extension 2.0.0** cannot sign through the desktop until it updates. A lost laptop on 2.0.0 is
  therefore cut off already.

## Formats

- **Shared record:** no new field. `clientSecretKey` in a pointer is optional already; the desktop no
  longer writes it. The pointer's `secret`, `null` until now, carries the browser secret.
- **Desktop record:** gains the browsers (per browser key its name, approved time, and a blocked
  mark) and each Desktop only identity's browser relays and trust. The desktop record is the desktop's own and can change here alone (`docs/PLAN.md`).
- **Browser:** its key stays in its local Gooti store (sealed with the Gooti password), never in the
  backup. Its device record could carry the code for Devices, but needs not: the code is derived.
- **NIP-46:** standard `connect` with client metadata. The desktop recognises a Gooti browser by a
  `gooti=browser` hint in the metadata and handles it as above instead of as a new app.

## What stays the same

- A lost laptop keeps what it has: the backup's contents up to then, including the keys of identities
  that are used in browsers. Only "Desktop only" identities are protected, and only after Disconnect.
- Other apps (NIP-46) are untouched. They already have one connection each, with Disconnect.

## Built

- Desktop: `bunker/handler.rs` (`browser_connect`), `bunker/store.rs` (allow, block, disconnect,
  trust, the retired shared key, browser relays, listening on pointer relays), `vault/desktop.rs` and `backup/desktop.rs` (the
  desktop record), Connections → Browsers and the approval window.
- Extension: `nip46/client.ts` (`connectGootiDesktop`), `background/background.ts`
  (`withBrowserKey`, `connectThisBrowser`), `services/storage/related/devices.ts`
  (`requestDesktopConnect`), `components/desktop-pairing`, the code in Devices.
- End to end: `e2e_extension.rs` `a_browser_with_its_own_key_is_allowed_signs_and_is_disconnected`
  with `tools/vectors/browserkey.ts`.

## Work, in order

1. **Desktop:** per-browser connections, the approval window for a browser `connect`, the Browsers
   section in Connections with Disconnect, blocked keys, the shared-key row, desktop record fields.
2. **Extension:** own key, "Connect this browser" with the name, the waiting state with the code,
   the code in Devices.
3. **Desktop:** stop writing `clientSecretKey`, and retire the shared key (2026-11-17).

Checks: unit tests on both sides, the extension-vector checks, and an end-to-end test in
`e2e_extension.rs` where a browser connects, signs, is disconnected, and fails.

## Decisions

1. **Trust level per identity,** the same for every browser, default Basic (changed 2026-11-17 from
   per browser: how careful to be depends on the key, and a lost browser is disconnected).
2. **The browser asks by itself only at the two moments** in step 2, never in the background.
3. **A request waits 5 minutes,** like every request to the desktop, then expires.
