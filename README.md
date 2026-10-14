# Star Citizen Rich Presence — Rust edition

A portable Discord Rich Presence companion for Star Citizen on Windows. Reads
`Game.log` without screenshots, Tesseract, `r_DisplayInfo`, or changes to `user.cfg`.

## What it shows

- Game startup, main menu, loading, and general in-game status.
- **Last reported location**, from local-player inventory requests and correlated
  quantum target/arrival events. Includes a bundled Stanton, Pyro, and Nyx alias catalogue.
- Ship name from vehicle-channel boarding/exit notifications.
- Friendly names for current ships, ground vehicles, and special variants.
- Active contract names/count from local HUD acceptance and outcome notifications.
- Short-lived blueprint, refinery-order, and hangar-ready notifications.
- Elapsed game session time and an optional public RSI profile button.

The parser was checked against local **4.10.197.2878** logs. An interactive
**4.10.2 LIVE** desktop smoke test also verified main-menu presence, Lorville
location reporting, the displayed Discord profile card, and cleanup on game exit.
Location, ship, contract, and quantum event shapes were also found in recent
4.10 backups. This is not a
continuous position API: the game does not log every movement. Presence says
**Last seen**, expires locations after 15 minutes by default, and uses a general
status for unknown locations. Selecting a quantum target alone never changes the
reported location. Mission-beacon IDs are not treated as named destinations.

## Run

Download/extract the portable Windows build, open Discord desktop, then launch
`star-citizen-rich-presence.exe`. The app waits for the game and clears presence
when it exits. Stop it with Ctrl+C. No administrator rights are normally needed.

Automatic discovery first tries the running game's executable, then standard
installation and AppData folders on mounted drives. Windows may hide the game's
executable path; use an explicit folder for a custom installation:

```powershell
.\star-citizen-rich-presence.exe --game-dir 'D:\Games\StarCitizen\LIVE'
```

More options:

```powershell
# Preview locally without publishing to Discord
.\star-citizen-rich-presence.exe --dry-run

# Select a testing channel
.\star-citizen-rich-presence.exe --channel EPTU

# List discovered paths without publishing anything
.\star-citizen-rich-presence.exe --discover

# Check Discord: briefly send a labelled test status, then clear it and exit
.\star-citizen-rich-presence.exe --check-discord

# Inspect a saved log once without connecting to Discord
.\star-citizen-rich-presence.exe --replay 'D:\Games\StarCitizen\LIVE\Game.log' --no-profile

# Hide profile links and contract names
.\star-citizen-rich-presence.exe --no-profile --no-contracts --no-notifications

# Explicit log (also supported through the SCRP_LOG environment variable)
.\star-citizen-rich-presence.exe --log 'D:\Games\StarCitizen\LIVE\Game.log'
```

`--replay` prints the final state; a log ending in shutdown correctly produces
`"presence": null`. Replay does not claim that a saved session is currently running.

## Current game-content catalogue

Version **2.1.0** embeds the **4.10.2-LIVE.12881860** extracted-game-data snapshot
from the [Star Citizen Wiki API](https://api.star-citizen.wiki/api/locations):

| Content | Records |
| --- | ---: |
| Stanton locations | 923 |
| Pyro locations | 582 |
| Nyx locations | 448 |
| Total named location records | 1,953 |
| Vehicle class/variant records | 310 |

This replaces the original twelve-location list and the rewrite's initial
65-entry catalogue. It covers named outposts, stations, distribution centres,
caves, contested-zone records, Nyx service and QV stations, and other starmap
points. Vehicle names include current Mk variants, Alliance and Wikelo variants,
and the newer models present in this snapshot. Contracts remain name-driven, so
new contract titles do not need a hardcoded mission list.

Records contain game UUIDs, unambiguous names, and parent-scoped aliases. The
resolver recognizes a settlement name inside a known parent's internal identifier
before falling back to its planet. Shared hierarchy tags and unqualified duplicate
names cannot silently select a different destination. UUIDs resolve precisely;
verified log-only identifiers are retained in `data/location-overrides.json`.

Extracted game data can contain hidden, placeholder, or future content. Placeholder
names are excluded. Recognizing a named record is **not a claim that it is currently
visitable**. Logs can still use an unmapped identifier: those events retain the
honest parent-area/general fallback. Neither the catalogue nor runtime makes up
location events or telemetry for game features that do not log them.

Blueprint/refinery/hangar notices last at most 60 seconds and can be hidden with
`show_notifications = false` or `--no-notifications`. A refinery completion can
occur remotely and never changes the reported player location. Respawn events
clear stale ship/location/incapacitation state without resetting elapsed time.

The app stays offline apart from local Discord IPC. Names, aliases, and vehicle
classes are embedded; it does not fetch game data during play. Provenance and
counts are in `data/content-source.json`. A developer can update the snapshot
using Node.js 20+ (not required to run the app):

```text
node scripts/refresh-content.mjs --version 4.10.2-LIVE.12881860
cargo test --locked
cargo build --locked --release
```

The refresh tool pins every page to one game version, validates completeness,
rejects alias collisions, and omits descriptions, imagery, and player data. Review
the source version assertion in `tests/content.rs` when deliberately updating
to a different patch. The catalogue is reproducible from cached API responses
with `--input-dir`; runtime never reads that cache.

## Settings and images

Copy `scrp.example.toml` to `scrp.toml` beside the executable, or pass `--config`.
CLI options override settings. Relative paths in TOML resolve against the settings
file's folder. LIVE, PTU, EPTU, and TECH-PREVIEW are supported.

The original Discord application ID remains the default. You can select your own
with `--app-id`. The displayed game name and images belong to that Discord
application. The default uses its existing `logo` asset; location art is opt-in in
`[image_assets]`, because new Pyro/Nyx assets cannot be uploaded to someone else's
application. Keys must exist in the chosen application's Rich Presence assets.

Aliases are embedded in the executable. To add or fix an identifier without
rebuilding, set `locations_file` to a JSON file like:

```json
[
  {"name":"My outpost","system":"Pyro","aliases":["EXACT_LOG_LOCATION_ID"]}
]
```

Custom aliases override matching built-ins. Matching is case-insensitive and
chooses the most specific alias with a token boundary. Unmapped sublocations can
fall back to a known parent area; arbitrary lines mentioning a planet are ignored.
The catalogue names more locations than the locally verified log identifiers;
Pyro/Nyx and other unobserved identifiers may need aliases as CIG changes logs.
It does not guarantee every outpost or patch-specific destination.

## Reliability and privacy

- Reads only: no game memory access, injection, screenshots, or config edits.
- Only Discord's local IPC pipe/socket is used; no remote telemetry service.
- Raw logs are never uploaded. Presence may expose location, ship, contract name,
  and a public profile link; disable the latter two through settings/CLI.
- Inventory locations require the authenticated local handle, so another player's
  inventory event cannot replace yours. Identity comes from the login-success log.
- Handles append, partial UTF-8 lines, missing files, truncation, and replacement.
- Bounded reads and line lengths prevent a large log from consuming unlimited RAM.
- Updates are deduplicated and throttled to at least 15 seconds. Discord response
  errors are checked; failed sends are retried, and a periodic refresh detects restarts.
- Session elapsed time survives Discord reconnects and server transitions.

If a game patch stops logging an event, that feature falls back rather than
inventing a location. A stale contract or ship can remain until its exit/outcome
event or a session transition; these log events are not a complete telemetry feed.

If the old Go version enabled `r_DisplayInfo`, this replacement leaves that setting
alone. Remove the old line from your game config yourself to hide that overlay.

## Build and validate

Rust **1.95+**, with a Windows linker. The recommended Windows setup is Rust's
MSVC toolchain plus Visual Studio Build Tools (Desktop development with C++).
The GNU toolchain also works with MinGW binutils and Rust's bundled linker:
set `RUSTFLAGS=-C linker=rust-lld` if your MinGW distribution lacks `libgcc_eh`.

```text
cargo fmt --all -- --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
cargo build --locked --release
```

The executable is in `target/release`. Parser/state/IPC tests also run on Linux;
automatic game discovery is intended for Windows. Linux/Wine installations should
use explicit paths, and their live process detection is not validated here.

GitHub Actions checks formatting, linting, tests, and builds. The release workflow
packages a Windows ZIP and SHA-256 checksum on a `v*` tag, or produces downloadable
build artifacts when run manually. No release has been published by this rewrite.

## Code map

| Module | Responsibility |
| --- | --- |
| `config` | CLI, TOML settings, validation |
| `discovery` | Running process and installation/log discovery |
| `tail` | Bounded, rotation-aware read-only log tail |
| `parser` | Explicit local-client log event extraction |
| `session` | Identity, lifecycle, ship, contracts, last location |
| `locations` | Embedded/override aliases and specific parent matching |
| `vehicles` | Current game vehicle class/localization names |
| `presence` | Privacy, payload fields, throttling, deduplication/retries |
| `discord` | Local IPC handshake, framed requests, acknowledged responses |

`tests/fixtures/session-4.10.log` is a synthetic, anonymized session using observed
4.10 event shapes, not an uploaded copy of a private game log.

## References

See [VALIDATION.md](VALIDATION.md) for the local verification record and its limits.

- [RSI: game log files](https://support.robertsspaceindustries.com/hc/en-us/articles/360000065688-Send-In-Game-Files-for-RSI-Support)
- [Discord: local IPC and SET_ACTIVITY](https://docs.discord.com/developers/topics/rpc)
- [SC Bridge: documented log event patterns](https://github.com/SC-Bridge/sc-companion/blob/main/docs/parser-patterns.csv)
- [Star Citizen Wiki: versioned location API](https://skmtc.dev/starcitizenwiki/apis/star-citizen-wiki-api/docs/api/locations/get)
- [Star Citizen Wiki: versioned vehicle API](https://skmtc.dev/starcitizenwiki/apis/star-citizen-wiki-api/docs/api/vehicles/get)
- [RSI: March 2026 report covering Stanton, Pyro, and Nyx](https://robertsspaceindustries.com/en/comm-link/transmission/21094-Star-Citizen-Monthly-Report-March-2026)

MIT licensed. Retains the original project's license and attribution. Unofficial
community tool, not affiliated with Cloud Imperium Games or Discord.
