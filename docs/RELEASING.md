# Releasing Gooti Desktop

Linux (AppImage, deb, rpm, and the AUR package `gooti-desktop-bin`) and Windows (an NSIS installer,
`Gooti_<version>_x64-setup.exe`, installed per user without admin rights). Builds are not
OS-code-signed, so Windows SmartScreen warns before the first start. Updates are signed with Gooti's own update key, which the app checks before installing.

## Once: the update key and the GitHub secrets

The key pair was made with `npx tauri signer generate` and lives outside the repo:

- `~/.tauri/gooti-desktop.key`: the private key. **Never commit it.** Keep a copy in your password manager.
- `~/.tauri/gooti-desktop.key.password`: its password. Same.
- `~/.tauri/gooti-desktop.key.pub`: the public key. It is also in `src-tauri/tauri.conf.json`
  (`plugins.updater.pubkey`); the app accepts only updates signed with the matching private key.

Losing the private key means installed AppImages can no longer update themselves: a new key needs a new
public key in the app, so users must download that version by hand once.

In the GitHub repository (Settings → Secrets and variables → Actions), add:

- `TAURI_SIGNING_PRIVATE_KEY`: the contents of `~/.tauri/gooti-desktop.key`
- `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`: the contents of `~/.tauri/gooti-desktop.key.password`

## Building on this machine

- `npm run build:local`: AppImage, deb and rpm in `target/release/bundle`, without update files. Needs
  no key. Enough to try a build (`src-tauri/tauri.local.conf.json` turns the update files off).
- `npm run build`: the same plus the signed update files, as the release workflow makes them. It needs
  the key in the environment (fish: `env TAURI_SIGNING_PRIVATE_KEY=(cat ~/.tauri/gooti-desktop.key | string collect) ...`):

  ```
  TAURI_SIGNING_PRIVATE_KEY="$(cat ~/.tauri/gooti-desktop.key)" \
  TAURI_SIGNING_PRIVATE_KEY_PASSWORD="$(cat ~/.tauri/gooti-desktop.key.password)" \
  npm run build
  ```

  Tauri does not read a `.env` file. Keep the key out of the repo.
- `npm run build:windows`: the Windows installer, cross-built on Linux, without update files, in
  `target/x86_64-pc-windows-msvc/release/bundle/nsis`. For trying it in a Windows VM; releases still
  come from GitHub. Tools it needs: `CLAUDE.md`, section Windows.

## Each release

Branches and what GitHub does with them:

| Branch | On push |
|---|---|
| `beta`, `feature/...`, others | Nothing. |
| `develop` | **Develop** workflow: tests, clippy, fmt, then the Linux bundles (deb, AppImage) and the Windows installer, without update files; on Mac only a compile check. Download them from the run's page under Artifacts (kept 14 days). |
| `main` | Nothing. Holds the release tags from 1.0.0 on. |
| tag `v*` | **Release** workflow (below). |

Pull requests start nothing.

Releases before 1.0.0 are tagged on `develop`. From 1.0.0 on, `develop` is merged into `main` and
the releases are tagged there. Below, "the release branch" is `develop` before 1.0.0 and `main` after.

1. On `beta`, once everything for the version is in, make the release commit, with every check in
   `CLAUDE.md` passing:

   ```
   tools/release/set-version.sh 0.2.0
   ```

   This sets the version in `Cargo.toml` (the app version), `package.json` and the lock files. In
   `CHANGELOG.md`, rename "Unreleased" to "0.2.0 (date)" and start a new, empty "Unreleased" above it.
   Commit both as "Release 0.2.0". The bump is the last commit on `beta`, so `beta`, `develop` and
   later `main` carry the same version and nothing has to be merged back.
2. Merge `beta` into `develop` (`git merge --no-ff beta`), push, and test the bundles from the Develop
   run. From 1.0.0 on, then merge `develop` into `main`. Tag the release branch and push:

   ```
   git tag v0.2.0
   git push origin develop v0.2.0     # from 1.0.0 on: main instead of develop
   ```

3. The **Release** workflow (`.github/workflows/release.yml`) checks that the tag matches the version
   and creates a **draft** release. Then, on Linux and on Windows, it runs `npm test`, builds the
   AppImage, deb and rpm, and the Windows installer, signs them for the updater, and adds each
   platform to the draft's `latest.json`.
4. Check the draft: `latest.json` must list both `linux-x86_64` and `windows-x86_64` (the two builds
   write it one after the other; if one is missing, rerun that build). Then publish it. Only a published release is "latest": installed AppImages see the
   update at their next start (when Check for updates is on), or with Settings → Check now.
5. AUR: update the package and push it to the AUR repository:

   ```
   packaging/aur/update.sh 0.2.0
   cd packaging/aur/gooti-desktop-bin
   # in a clone of ssh://aur@aur.archlinux.org/gooti-desktop-bin.git: copy PKGBUILD and .SRCINFO, commit, push
   ```

   `update.sh` downloads the release's .deb, writes its checksum into `PKGBUILD`, and regenerates
   `.SRCINFO`. Commit the changed `PKGBUILD` here too.

## How updates reach people

- **AppImage**: Gooti downloads the new AppImage, checks its signature against the public key, replaces
  the file, and restarts.
- **Windows**: Gooti downloads the new installer, checks its signature, starts it (passive: a progress
  bar, no questions) and quits. The installer starts Gooti again.
- **deb, rpm, AUR**: Gooti only says that a version is out and links to the release. The package manager
  installs it, so Gooti never runs `dpkg` or `rpm` itself.

## Testing an update locally

A release build refuses a plain `http://` update address, so build the old version for the test only with
`--config '{"plugins":{"updater":{"dangerousInsecureTransportProtocol":true}}}'`. Then serve a newer, signed
AppImage and a `latest.json` (`version`, and `platforms.linux-x86_64-appimage` with `signature` and `url`)
from a local web server, and start the old AppImage with `GOOTI_UPDATE_URL=http://127.0.0.1:PORT/latest.json`.
`GOOTI_UPDATE_URL` only changes where Gooti looks; the update must still carry the release signature.
