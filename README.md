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

## Apps (NIP-46)

- **Methods:** `connect`, `get_public_key`, `sign_event`, `nip04_encrypt`, `nip04_decrypt`,
  `nip44_encrypt`, `nip44_decrypt`, `ping`, `switch_relays`, `logout`.
- **Connecting:** Connections → **Connect new app**. Either show a `bunker://` link with a QR code
  (single use, valid for one hour), or paste a `nostrconnect://` link. A `nostrconnect://` link clicked
  in a browser or another app opens this page with the link filled in; you still choose the identity
  and press Connect.
- **Privacy:** each identity signs for apps with its own signer key, so relays cannot tell which
  identity an app uses.
- **Gooti browsers:** a Desktop only identity gets one connection named "Gooti browsers" for all your
  browsers with the extension. Requests from it name the site ("primal.net through Gooti browsers").

## Permissions and activity

- **Trust level per app:** *Ask every time*, *Basic* (notes, reactions, messages and the like are
  approved; changes to your profile, follow list, relays, deletions, sign-ins and wallet requests still
  ask), or *Trust* (everything approved).
- **Remembered choices:** in the request window, Approve or Reject only this time, for 5 minutes, an
  hour, a day, or always. Sensitive kinds are remembered for an hour at most. A remembered Reject
  always wins. Each app's page lists its remembered choices, and you can forget them.
- **Requests** show in a small window and as a notification. Unanswered requests time out after
  5 minutes. **Pause all apps** answers every app with an error until you turn it off.
- **Activities:** requests, connects, removals and backups, newest first (the latest 500). Filter by
  app or backups; each app's page shows its recent activity. Activity stays on this desktop and is not
  backed up.

## Relays

- **Default relays for apps** (Settings), starting with `wss://relay.getgooti.com`. They fill new
  bunker links, new Desktop only identities, and "Move key to desktop". Each of those keeps its own
  copy, so changing the default changes nothing in use.
- **Each app keeps its relays:** the ones its link named. Gooti browsers' relays can be changed on
  their page; backup takes the change to the browsers.
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

LICENSE: Public Domain
