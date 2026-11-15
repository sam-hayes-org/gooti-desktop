# Changelog

What changed in each version of Gooti Desktop. The newest version is at the top. What Gooti Desktop
does in full is in `README.md`.

## Unreleased

- Backup: a deleted identity leaves a row with no private key and no bunker secret in the backup.
  Read this only with Gooti Desktop 0.1.3 or later and extension 2.0.0 or later; older versions
  reject such a row.

## 0.1.3 (2026-11-15)

- Backup: Gooti reads a deleted identity that has no key or bunker pointer in its row. A later
  Gooti keeps no secrets in such a row. This version still writes the key, so older versions can
  read what it sends.
- Backup: a backup made by a newer Gooti is no longer replaced by this version's older document.
  Gooti says "This backup was made by a newer Gooti. Update Gooti first." and leaves it alone.
- Backup: when a relay does not answer and the others have no backup, Gooti no longer takes that
  for "no backup" and sends its own vault over a backup the silent relay may hold. It reports the
  relay instead.
- Building: `npm run build:local:appimage` (also `:deb`, `:rpm`) builds one format with a faster
  Cargo profile (`local`): about a minute after a small change instead of about eight.

## 0.1.2 (2026-11-13)

- License: Gooti Desktop is now under the MIT License (0.1.0 and 0.1.1 were released under the
  Unlicense).
- Info → **Open-source licenses** shows the licenses of everything Gooti ships with. Every bundle
  carries them in `licenses/`, and the AUR package installs the MIT license where Arch expects it.
- The side panel no longer shows the channel and version under Lock Gooti; Info shows them. When an
  update is out, the side panel names both versions ("Gooti 0.1.2 is available. You have 0.1.1.").

## 0.1.1 (2026-11-13)

- Building: npm no longer warns about the install scripts of four UI build tools; they are
  approved in `ui/package.json`.
- Building: the password field no longer causes an Angular warning in the tests.
- Releasing: before 1.0.0, releases are tagged on `develop` (`docs/RELEASING.md`).

## 0.1.0 (2026-11-13)

The first alpha, for Linux (AppImage, deb, rpm) and Windows (installer, not yet code-signed).

- **Identities:** create a key on the desktop, or import an nsec, an ncryptsec or a hex key. Show the
  nsec or export an ncryptsec. Per identity, choose where the key is used: **Browsers and desktops**
  or **Desktop only**.
- **Apps (NIP-46):** connect an app with a `bunker://` link and QR code, or with a `nostrconnect://`
  link (pasted, or clicked in a browser). Every NIP-46 method, including `logout`. Each identity
  signs with its own signer key. Gooti browsers sign for Desktop only identities without a link.
- **Permissions:** Ask every time, Basic or Trust per app; choices remembered for 5 minutes up to
  always; a request window and notifications; Pause all apps; Activities per app.
- **Backup:** one 12-word passphrase shares your identities with Gooti in browsers and on other
  Gooti Desktops, encrypted on your relays. Back up automatically, or Back up now. Restore from
  backup when setting up Gooti. Connections mark the apps another Gooti Desktop answers.
- **Locking:** Gooti password; optional unlock when Gooti starts through the system keychain; lock
  after idle time, when the screen locks and before the computer sleeps.
- **System:** tray, start at login, signed self-updates (AppImage and Windows), keyboard shortcuts.
