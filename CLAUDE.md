# gooti-desktop

Gooti Desktop: a Nostr identity vault and NIP-46 bunker for Linux (first), later Mac and Windows.
It belongs with the Gooti browser extension in `../gooti-extension`. The plan and the decisions made so far are in
`docs/PLAN.md`. Read it before starting a milestone.
Sites and apps for testing by hand are in `docs/TESTING.md`.

**Keep `README.md` up to date.** It lists what Gooti Desktop does: features, NIP-46 methods, defaults,
relays, backup, security, shortcuts, and limitations. When a change adds, removes, or changes any of
these, update the README in the same commit.

## Layout

- `crates/gooti-core`: all logic that touches keys (vault, backup, bunker, permissions). It has no UI and no Tauri,
  so a later server app can reuse it.
- `src-tauri`: thin Tauri 2 shell (windows, tray, commands). Commands wrap `gooti-core` and hold no logic.
- `ui`: Angular 22 app. It reaches Rust only through `ui/src/app/core/core.service.ts`.
- `assets/app-icon.svg`: source for `src-tauri/icons` (`npm run icons`).

**Private keys never cross into the UI.** The only exception is an explicit "Show nsec" action.
A generated key is made in the core; the UI only sends a key the user pasted to import.

- `src-tauri/src/ipc_tests.rs` sends the exact JSON that `core.service.ts` sends. Change both together.
- `ui/src/app/identities/appearance.ts` is a copy of the extension's avatar and color code, so an identity
  looks the same in both. Keep them in step.
- Errors carry `code` (for logic) and `message` (shown to users). Messages live in
  `crates/gooti-core/src/error.rs` and follow the wording rules below.

## Commands

```
npm install            # root: Tauri CLI. Also runs `npm ci` in ui/
npm run dev            # starts ng serve on port 1420 and the app
npm run build:local    # Linux bundles in target/release/bundle, without update files (no key needed)
npm run build          # the same plus signed update files; needs the update key (docs/RELEASING.md)
npm run build:windows  # the Windows installer, cross-built on Linux (see Windows below)
npm run start:local:latest  # starts the newest AppImage from that folder (-- --which: only print it)
npm test               # cargo test --workspace, then the UI tests
cargo clippy --workspace --all-targets -- -D warnings
```

Linux build needs: `rustup`, `webkit2gtk-4.1`, `libayatana-appindicator`, `librsvg`.

### Windows

There is no Windows machine here. `npm run check:windows` runs clippy for Windows and every test as a
Windows binary under Wine (`-- --ignored` adds the keychain test). It needs `mingw-w64-gcc`, `wine`
and `rustup target add x86_64-pc-windows-gnu`. Run it after any change to code under `cfg(windows)` or
`cfg(target_os = "linux")`, to Tauri plugins, or to the IPC tests. GitHub builds and tests on real
Windows when `develop` is pushed, and makes the release installer. Windows serves the UI from
`http://tauri.localhost`, not `tauri://localhost`.

`npm run build:windows` builds the installer here, for trying it in the Windows VM
(`target/x86_64-pc-windows-msvc/release/bundle/nsis/`). It needs `cargo-xwin` (`cargo install
cargo-xwin --locked`, lands in `~/.cargo/bin`, which the script adds to PATH),
`rustup target add x86_64-pc-windows-msvc`, clang, lld, llvm and wine. It runs the official NSIS for
Windows under Wine (`tools/windows/makensis`), because the AUR `nsis` makes 64-bit installers only and
Tauri's installer script and plugin are 32-bit.

### Checks

After **any** change:

```
npm test
cargo clippy --workspace --all-targets -- -D warnings
```

After a change to **backup, the vault, identities, custody, or the bunker** (`crates/gooti-core/src/backup/`,
`vault.rs`, `identity.rs`, `bunker/`), also run the checks against the extension's real code. They need
`../gooti-extension` with its `node_modules`, so they are not part of `npm test`:

```
npm run vectors:check                                        # the extension opens a vault Rust sealed
cargo test -p gooti-core --test e2e_extension -- --ignored   # extension backup, bunker client, custody, end to end
```

Do not report such a change as done unless these pass. They caught bugs the unit tests did not.

On request, or after touching that area:

```
cargo test -p gooti-desktop -- --ignored                       # the real OS keychain (needs a Secret Service)
cargo test -p gooti-core --test relay_websocket -- --ignored   # wss:// to relay.getgooti.com (read only)
cargo test -p gooti-core --test bunker_flow -- --ignored       # bunker over relay.getgooti.com (publishes kind 24133)
npm run build:local                                            # a release build; check it looks right
```

When the extension's backup code changes, regenerate its test values with `npm run vectors` and commit
`crates/gooti-core/tests/vectors/extension.json`.

The dev app uses the real data folder `~/.local/share/com.getgooti.desktop` (vault.json, settings.json).

## Things that cost time before

- **Release builds without styles.** Angular adds component styles at runtime as inline `<style>`. In
  release builds Tauri adds a nonce to the CSP's `style-src`, and with a nonce present the browser ignores
  `'unsafe-inline'`, so those styles were blocked (dev mode looked fine). `tauri.conf.json` therefore sets
  `dangerousDisableAssetCspModification: ["style-src"]`. Check a release build (`npm run build:local`), not only
  `npm run dev`, after touching the CSP.

## Compatibility with the extension

The shared record must match the extension byte for byte (details in `docs/PLAN.md`). Do not change the
format here alone. A change needs the same change in `../gooti-extension/projects/common/src/lib/backup`.
Do not edit the extension repo unless asked. Its owner commits there.

The **desktop record** (`crates/gooti-core/src/backup/desktop.rs`) is the desktop's own: the extension
never reads it, so it can change here alone. It holds what only Gooti Desktops may see (Desktop only
keys, bunker signer keys, connections). See "Design: the desktop record" in `docs/PLAN.md`.

## Look

The UI must look like the extension. The tokens and the Inter font are in `ui/src/styles.scss`. The files in
`ui/src/styles/gooti/` are copies of `gooti-extension/projects/common/src/lib/styles`. Keep them in step, and put
desktop-only layout in `ui/src/styles/_desktop.scss`.

**No system UI inside the window.** The app must look the same on Linux, Mac and Windows, so the system
draws nothing in it:
- Use `<app-select>` (`ui/src/app/shared/select.ts`), never `<select>`.
- Use `appTooltip="..."`, never `title="..."`.
- `<app-text-menu>` replaces the right-click menu. It's built on the Angular CDK (overlay, listbox, menu).
- Checkboxes, radio buttons, focus rings, scrollbars and the engine's extras in text fields are styled in
  `ui/src/styles/_controls.scss`.

Only the tray menu, notifications and the window frame stay system.

## Wording (same rules as the extension)

- Sentence case for headings and buttons ("New identity", "Delete identity").
- Call the 12 words a **passphrase**, never "recovery phrase" or "phrase".
- Short confirmations: "Delete this identity?", "Lock Gooti?".
- **Backup** is what the passphrase opens on the relays: "Turn on backup", "Back up now", "Back up
  automatically", "Restore from backup", "Use an existing backup", "Backup size". Never "sync" or
  "vault" in text users see. One sentence explains the sharing, in words that name Gooti (not just
  any browser): "Enter the same passphrase in Gooti on a browser or on another Gooti Desktop, and they share your identities."
- No noun for the local store: "Lock Gooti?", "Unlock Gooti", "Gooti password", "Set up Gooti". Where
  it matters, say "on this browser" or "on this desktop": each device has its own Gooti password.
- "Lock" and "Unlock", not "Sign in" and "Sign out".
- Say "relay", not "sync relay".
- Code uses the same words: `backup` (module, commands, events), `shared` (the record browsers read,
  `SharedDocument`), `desktop record`. `vault` stays for the local store in code (`vault.rs`), as users
  never see it. Data stored before 2026-11-12 is read through serde aliases (`synced`, `sync`).
- Custody is "Where the key is used": **Browsers and desktops** or **Desktop only**. The actions are
  "Move key to desktop" and "Use in browsers again".

## Git

Work on the `beta` branch, and commit there after a verified step. Leave `main` and `develop` alone unless
asked. GitHub runs nothing for `beta`: a push to `develop` builds test bundles, a `v*` tag on `main` builds
a release (`docs/RELEASING.md`).
