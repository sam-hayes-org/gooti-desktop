# Gooti Desktop

A Nostr identity vault and NIP-46 remote signer ("bunker") for the desktop. It belongs with the
[Gooti browser extension](https://github.com/sam-hayes-org/gooti-extension): both share your identities
through one encrypted backup, and the extension can sign through the desktop so a key never has to
leave it.

**Status: alpha.** Linux (AppImage, deb, rpm, AUR) and Windows (installer, not yet code-signed, so
Windows warns before the first start). macOS later.

Built with Tauri 2: a Rust core (`crates/gooti-core`) holds every key and all logic, a thin shell
(`src-tauri`) adds windows, tray and system integration, and an Angular UI (`ui`) shows it. Private keys
never reach the UI, except when you ask to show an nsec.

## Identities

- **Create or import:** generate a new key on the desktop, or import an nsec, an ncryptsec (NIP-49) or
  a hex key. Each identity has a nick and a color, the same as in the extension.
- **Where the key is used:** per identity, **Browsers and desktops** (the backup carries the key to
  every Gooti with your passphrase) or **Desktop only** (browsers never get the key; they sign through
  this desktop). New identities are Desktop only unless you untick it. "Move key to desktop" and
  "Use in browsers again" switch later.
- **Keys:** show the nsec on request, or export an ncryptsec with a password of your choice.
- **Site-only identities:** an identity a browser made with "New site-only identity", for testing a
  site and throwing it away later. Identities lists them in their own group, "Site-only identities", below "Regular identities" (both open; each can be closed; an info icon explains them). Its
  page names the site and offers **Keep as a regular identity**, which turns it into a regular one on
  every device. Delete it like any identity.

## Apps (NIP-46)

- **Methods:** `connect`, `get_public_key`, `sign_event`, `nip04_encrypt`, `nip04_decrypt`,
  `nip44_encrypt`, `nip44_decrypt`, `ping`, `switch_relays`, `logout`.
- **Error codes:** an error reply has a `code` next to `error`, for programs: the text is written for
  people and may change, the code does not. `not_connected` (this client key has no connection: never
  connected, disconnected here, or logged out; connect again), `link_invalid`, `paused`, `rejected`,
  `timed_out` (nobody answered within 5 minutes), `unsupported_method`, `invalid_params`, `failed`,
  and for Gooti browsers `browser_disconnected`, `browser_not_known`, `browser_update`. A client key
  gets a reply only while this desktop still listens on that identity's signer key and relay: after the
  last app of an identity that is not Desktop only is disconnected, or while Gooti is locked, there is
  no answer at all.
- **Connecting:** Connections → **Connect new app**. Either show a `bunker://` link with a QR code
  (single use, valid for one hour), or paste a `nostrconnect://` link. A `nostrconnect://` link clicked
  in a browser or another app opens this page with the link filled in; you still choose the identity
  and press Connect.
- **Privacy:** each identity signs for apps with its own signer key, so relays cannot tell which
  identity an app uses.
- **Browsers with their own key** (`docs/DESIGN-browser-keys.md`): a newer extension asks once, with
  its own key that never goes into the backup. A window asks **Allow this browser?** with its name and
  a short code the browser shows too; Allow covers every Desktop only identity, Deny blocks the key for 90 days.
  The request carries a secret from the backup, so someone who only sees this desktop's signer key
  on a relay cannot ask.
  Connections → **Browsers** lists each browser (name, code, when allowed, last used) with
  **Disconnect**, which stops its key at once, on every Gooti Desktop. How far browsers may go without
  asking is set per identity, on a Desktop only identity's page under **Browsers** (default Basic), the
  same for every browser. That section also lists the choices remembered for each browser, with
  **Forget**. Requests from a
  browser name the site ("primal.net through Work laptop"). There is no shared browser key: backup
  carries none, and a browser on Gooti 2.0.0 is told to update.

## Permissions and activity

- **Trust level per app:** *Ask every time*, *Basic* (notes, reactions, messages and the like are
  approved; changes to your profile, follow list, relays, deletions, sign-ins and wallet requests still
  ask), or *Trust* (everything approved).
- **Remembered choices:** in the request window, Approve or Reject only this time, for 5 minutes, an
  hour, a day, or always. Sensitive kinds are remembered for an hour at most. A remembered Reject
  always wins. Each app's page lists its remembered choices, and you can forget them.
- **An app that asks again:** a site that stops waiting and sends the same request again (Primal gives up
  after about 8 seconds) opens no second window. The window says "Asked 3 times while waiting", and
  one Approve or Reject answers every copy, each with its own signature.
- **Requests** show in a small window and as a notification. Unanswered requests time out after
  5 minutes. **Pause all apps** answers every app with an error until you turn it off.
- **Activities:** requests, connects, removals and backups, newest first (the latest 500). Filter by
  app or backups; each app's page shows its recent activity. Activity stays on this desktop and is not
  backed up.

## Relays

- **Default relays for apps** (Settings), starting with `wss://relay.getgooti.com`. They fill new
  bunker links, new Desktop only identities, and "Move key to desktop". Each of those keeps its own
  copy, so changing the default changes nothing in use.
- **Each app keeps its relays:** the ones its link named. The relays browsers use for a Desktop only
  identity can be changed on its page under **Browsers**; that changes them for all its browsers, and
  backup takes the change to them.
- **Listening on** (Connections) shows every relay in use, its state, and which apps use it. The
  sidebar shows how many are connected (green, amber, red).
- `ws://` is accepted only for a relay on this computer (`localhost`, `127.0.0.1`, `[::1]`).

## Backup

- **One passphrase** (12 words) opens an encrypted backup on your backup relays. Enter it in Gooti on a
  browser or on another Gooti Desktop, and they share your identities.
- **Two records,** both replaceable kind 30078 events, sealed with AES-256-GCM under keys derived from
  the passphrase: the *shared record* that browsers and desktops read (identities, site choices,
  permissions), and the *desktop record* only Gooti Desktops can open (Desktop only keys, signer keys,
  app connections with their trust levels, relays and remembered choices).
- **Back up automatically:** on unlock, after each change, and every 5 minutes. **Back up now** runs
  one at once. Restore from backup is offered when setting up Gooti.
- **Two Gooti Desktops on one passphrase:** only one answers the apps of an identity. A desktop set up
  from the backup answers none until you choose **Sign here** (Identities) or **Sign here instead**
  (on an identity's or an app's page); the other desktop stops at its next backup. Connections marks
  each app another Gooti Desktop answers.
- **Devices** (on the Backup page): every Gooti that uses this backup, browsers and desktops, with
  when each last backed up. Each one publishes a small *device record* (`d` = `gooti-device:<id>`,
  sealed with the shared record's keys): its name, whether it is a browser or a desktop, and the time
  of its last backup, at most once an hour. **Rename** names this desktop ("Gooti Desktop on Linux"
  until then). **Remove from list** takes another device off the list; it comes back with its next backup, which
  reads its own record along with the shared record and publishes it again when it was removed. It
  does not lock that device out: it still holds the passphrase.

## Locking and security

- **Gooti password:** the local vault is sealed with XChaCha20-Poly1305 under a key from Argon2id
  (64 MiB, 3 passes). Data lives in `~/.local/share/com.getgooti.desktop` on Linux and
  `%APPDATA%\com.getgooti.desktop` on Windows. Uninstalling on Windows keeps it, unless you tick
  "Delete the application data".
- **Unlock when Gooti starts** (optional): the system keychain (Secret Service on Linux, Credential
  Manager on Windows) keeps the key to the
  vault, so anyone who can use your login can unlock Gooti.
- **Locking:** after idle time (never, 5 or 15 minutes, 1 or 4 hours), when the screen locks, and
  before the computer sleeps (both on by default), or with **Lock Gooti** (Ctrl+L). Locking forgets
  every private key held in memory and stops the bunker: apps cannot sign while Gooti is locked.
- **Forgotten Gooti password:** reset Gooti and restore from your backup passphrase. Without backup,
  keys that lived only on this desktop are lost.

## System integration

- **Tray:** Open Gooti, Lock now, Pause all apps, Quit (on Windows: right click; a left click opens
  the window). Closing the window keeps Gooti running in the tray (configurable).
- **Start Gooti when you log in** (optional): starts in the tray, without the window.
- **Updates:** checks GitHub at start (configurable). The AppImage and the Windows version install
  signed updates themselves; deb, rpm and AUR update through the package manager.
- **Links:** registered for `nostrconnect://` (installed packages and the Windows installer, or the
  AppImage once started).

## Interface

- Looks like the extension: same colors, Inter font, own controls instead of the system's, so it looks
  the same on every platform. Dark theme, English.
- **Keyboard shortcuts** (⌘ on macOS):
  - Ctrl+1 … Ctrl+6: Identities, Connections, Activities, Backup, Settings, Info.
  - Ctrl+L: lock Gooti.
  - In the request window: Ctrl+Enter approves, Ctrl+Shift+Enter rejects.
- **Info:** version, website, source code, shortcuts, and **Open-source licenses**: Gooti's MIT
  License and the licenses of everything it ships with (also in every bundle, in `licenses/`).

## Build and test

Arch and CachyOS:

```
sudo pacman -S --needed rustup webkit2gtk-4.1 libayatana-appindicator librsvg
rustup default stable
```

Then:

```
npm install
npm run dev                  # run from source (uses the real data folder)
npm run build:local          # AppImage, deb and rpm in target/release/bundle, without update files
npm run build:local:appimage # only the AppImage (also :deb, :rpm), faster to rebuild, in target/local
npm run start:local:latest   # start the newest local AppImage
npm run build:windows        # the Windows installer, cross-built (tools: CLAUDE.md, Windows)
npm test                     # Rust and UI tests
```

More checks (against the extension's own code, real relays, the OS keychain) are in `CLAUDE.md`.
Releasing is in `docs/RELEASING.md`, the design and decisions in `docs/PLAN.md`, and sites and apps
for testing by hand in `docs/TESTING.md`.

## Limitations

- Linux and Windows only for now. The Windows version is not code-signed yet: SmartScreen warns
  before the first start (More info → Run anyway).
- On Windows, computers with Modern Standby may not report sleep; there Gooti locks with the screen
  lock that comes with it.
- While unlocked, keys are in memory, so malware running as you could read them.
- Copied secrets (nsec, links) can be read by other apps through the clipboard.
- No Tor, no local relay, no command-line signer (NIP-5F), no `decrypt_zap_event` (not part of NIP-46).

---

License: MIT (see `LICENSE`). Versions 0.1.0 and 0.1.1 were released under the Unlicense.
