# Changelog

What changed in each version of Gooti Desktop. The newest version is at the top. What Gooti Desktop
does in full is in `README.md`.

## Unreleased

## 0.3.0 (2026-11-24)

- **Update every Gooti Desktop that uses your passphrase.** Gooti Desktop 0.2.0 does not know AI
  assistants: it shows an assistant's connections as separate apps, cannot answer when the assistant
  lists or creates identities, and leaves assistants out of the backups it writes.
- Connections: **AI assistants**. AI assistants such as Claude or Grok connect through Gooti MCP
  (https://mcp.getgooti.com) and act as your identities without ever getting a key. A link from
  Gooti MCP opens **Connect an AI assistant**: who asks, which identity it starts with, and what it may
  use: only that identity, or also every Desktop only identity, including ones you add later and ones
  it creates. Each assistant is listed with that choice (change it any time), its identities,
  **Rename** and **Disconnect**. An assistant asks before each request at first, like any app, and
  always before it creates an identity (window **Create a new identity?**). Assistants reach your other
  Gooti Desktops through the backup.
- Connections: the page lists **Browsers**, **Apps**, then **AI assistants**; **Connect new app**
  sits with Apps. An assistant's identities are colored by how far it may go without asking (grey Ask
  every time, blue Basic, amber Trust), and say so on hover. The number in the sidebar counts each
  app, assistant and browser once.
- Identities: an identity's page lists its **Apps** and **AI assistants** with the same colors; each opens
  the page where you set its trust.
- Every page has clearer section headings, with an icon.
- Window: it can be as low as 400 pixels. Tiling window managers such as Hyprland ignore the minimum
  height, and the bottom of the window (Lock Gooti, the end of long pages) was cut off.
- Apps: an error reply carries a stable `code` next to the error text (`not_connected`, `rejected`,
  `timed_out`, `paused`, and more; list in `README.md`), so programs no longer have to match the
  text. The texts stay the same, and apps that do not know the code ignore it.
- Apps: a disconnected app now learns so on the relay it uses. The answer to a client without a
  connection went only to the identity's browser relays; now it goes to the relays of the identity's
  apps too.
- Building: `npm run dev` unlocks in about a quarter of a second instead of about 5 seconds (Argon2 is
  optimized in dev builds too).

## 0.2.0 (2026-11-20)

- **Update every Gooti Desktop that uses your passphrase.** Gooti Desktop 0.1.4 still hands out the
  shared browser key that this version retires, so the two would keep changing the same backup
  records back and forth. Browsers need extension 2.1.0 to sign through this version.
- Connections: **Browsers**. A browser with Gooti extension 2.1.0 signs through this desktop with its
  own key, which never goes into the backup. It asks once; a window shows its name and a short code
  (**Allow this browser?**), and Allow covers every Desktop only identity, also ones made later. Each
  browser is listed with its code, when it was allowed, when it was last used, and **Disconnect**: its
  key stops working at once, on every Gooti Desktop with the passphrase. A denied or disconnected
  browser stays blocked for 90 days. Only a browser that opened
  your backup can ask: it sends a secret that travels sealed in the backup.
- Identities: a Desktop only identity's page has **Browsers**: how far your browsers may go as that
  identity without asking (default Basic), the choices remembered for each browser (with **Forget**),
  and the relays they reach it on. Trust and relays are the same for every browser.
- The shared browser key is gone. Gooti Desktop 0.1.4 put one key for all browsers into the backup, so
  any browser with the passphrase, a lost one too, could sign as a Desktop only identity. Backup no
  longer carries it and this desktop no longer accepts it: a browser on extension 2.0.0 is told to
  update Gooti. Its connection, "Gooti browsers", is removed when you unlock; the relays browsers use
  for each Desktop only identity are kept.
- Backup: **Devices** on the Backup page lists every Gooti that uses this backup, browsers and
  desktops, with when each last backed up. Each device publishes a small sealed record after a backup,
  at most once an hour (`d` = `gooti-device:<id>`). **Rename** names this desktop; **Remove from list**
  takes another device off the list until its next backup.
- Identities: **site-only identities**, made by Gooti in a browser for one site, are shown in their own
  group, "Site-only identities", apart from your regular identities. An identity's page says which site
  it was made for and offers **Keep as a regular identity**. Desktop 0.1.4 and extension 2.0.0 may
  drop the mark when they write the backup; the identity then shows as a regular one.
- Approval window: a site that stops waiting and sends the same request again (Primal gives up after
  about 8 seconds) no longer queues one window per attempt. The window shows the request once, with
  "Asked 3 times while waiting", and your answer goes to every attempt, so the site gets it at once.
- Info: the version is shown without "(alpha)".

## 0.1.4 (2026-11-15)

- Backup: a deleted identity leaves a row with no private key and no bunker secret in the backup.
  Gooti reads such a row, and the extension 2.0.0 does too.
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
