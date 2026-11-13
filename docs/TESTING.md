# Testing by hand

Sites and tools for trying the Gooti extension and Gooti Desktop against real Nostr apps. The automatic
checks are in `CLAUDE.md`. This list comes from memory (2026-11-12): check that a site still works before
relying on it.

Test notes are real and public on Nostr relays. Use throwaway identities, and delete test notes afterwards.

## Sites that use the extension (NIP-07)

| Site | Good for testing |
|---|---|
| primal.net | Notes, deletions. Gives up after about 8 seconds, so it shows slow answers |
| snort.social | Notes, reactions, deleting notes (kind 5), DMs with NIP-04 and NIP-44 |
| coracle.social | Many requests in a row, NIP-44 encryption, relay lists (kind 10002) |
| iris.to | Notes and DMs. Asks a lot right after login: good for permissions |
| nostrudel.ninja | Almost every event kind: unusual kinds and deletions |
| habla.news | Long articles (kind 30023), which have their own prompt |
| zap.stream | Live streams, chat, zap requests (kind 9734) |
| nostr.band | Search. Asks only for the public key: good for choosing a site identity |
| nostter.app | A simple client with few requests: clean step-by-step tests |
| jumble.social | A newer client that is strict about NIP-07 |

## Apps that connect to Gooti Desktop (NIP-46)

| App | How it connects |
|---|---|
| coracle.social | Login with a bunker link (`bunker://`) or Nostr Connect (`nostrconnect://`) |
| nostrudel.ninja | Both link types |
| snort.social | Bunker link |
| Amethyst (Android) | Nostr Connect, from the phone to the desktop |
| nak (command line) | `nak event --sec 'bunker://…' -c "test"`: scripted tests without a browser |

Get a bunker link from Connections → "Connect new app" → "Show bunker link". It works once, for one hour.

A `nostrconnect://` link clicked in a browser (or `xdg-open 'nostrconnect://…'`) opens Gooti on
"Connect new app" with the link filled in. Only an installed package or a started AppImage registers
Gooti for these links; `npm run dev` does not.

## What to try

1. **Site identities.** Log in to snort with one identity and to coracle with another. The Sites tab
   lists both. Remove one, and its next request asks for an identity again.
2. **Desktop only identity in the browser.** No browser window appears. The desktop's approval window
   names the site ("notes.example.com through Gooti browsers").
3. **Deleting a note** (snort or nostrudel). A Gooti Desktop identity: no browser window, the desktop
   decides. Any other bunker: the browser asks first.
4. **DMs** (coracle or iris). NIP-04 and NIP-44 encrypt and decrypt.
5. **An app straight against the desktop** (coracle with a bunker link, or nak). In Connections, check
   its trust level, the remembered answers, and Remove.
6. **Backup between devices.** Turn on backup in a browser, use the same passphrase on the desktop (or
   another browser), and check that identities, site choices and permissions arrive.
7. **Locked desktop.** Lock Gooti Desktop and sign in the browser with a Desktop only identity. The
   browser reports that Gooti Desktop did not answer.
