# Validation of the Rust rewrite

Validated on Windows on 14 October 2026. The portable application is built from
the Rust sources in this checkout; the legacy Go/OCR implementation was removed.

| Requirement | Evidence |
| --- | --- |
| Rust replacement | Cargo manifest, lockfile, Rust modules; Windows executable builds successfully |
| No `r_DisplayInfo` dependency | Runtime reads only process metadata and `Game.log`; no screenshot/OCR library or game configuration writer |
| Current Star Citizen event support | Observed location-inventory, ship-channel, contract HUD, and quantum events in local 4.10 logs; fixture retains their shapes with invented identities |
| Modern locations | Embedded Stanton/Pyro/Nyx catalogue, specific alias matching, additional aliases through settings; unobserved identifiers remain explicitly qualified in README |
| Discord publishing | Real Discord desktop accepted the default application ID, acknowledged a labelled test presence, and acknowledged clearing it through `--check-discord` |
| Safe attribution and lifecycle | Tests cover local-player inventory filtering, selection versus arrival, vehicle correlation, unknown destinations, teardown, and timestamp preservation |
| Log reliability | Tests cover partial UTF-8 lines, truncation, replacement, disappearance/recreation, and oversized lines |
| Discord recovery | Tests cover deduplication, rate limits, failure/reconnect; isolated real Windows pipes exercise handshake, PING/PONG, command acknowledgements, unrelated events, and rejection |
| Privacy controls | Replay integration test confirms profile opt-out; contract visibility is configurable; no raw log upload path exists |
| Usable deliverable | Portable Windows executable, ZIP, settings example, README, MIT license, and SHA-256 checksum |

Local gates for version 2.1.0: formatting check, Clippy with warnings denied, 29 tests, release
build, packaged executable discovery/replay, and actual Discord connection check.
GitHub CI/release workflows are provided but have not been executed remotely.

The real Discord diagnostic publishes only a labelled integration-test status
and clears it immediately. Gameplay parsing is also covered by saved logs and
the anonymized integration fixture.

## Interactive desktop smoke test

On 14 October 2026, used the Computer Use plugin to launch the installed RSI
Launcher, start **LIVE 4.10.2-live.12913470**, and enter the persistent universe.
The packaged companion ran with `--no-profile --no-contracts`.

- Automatically detected the game and its LIVE log.
- Observed startup and loading updates in the companion output.
- Visually confirmed **In the main menu**, the logo, LIVE label, and elapsed
  timer on the real Discord profile card.
- Entered a live session in the player's Lorville hangar with no debug overlay
  visible. The companion reported **Last seen: Lorville**.
- Visually confirmed **Last seen: Lorville**, **Stanton · LIVE**, the logo, and
  the continuing elapsed timer on the Discord profile card.
- Closed the game through the normal Windows close shortcut. The companion
  reported **Presence: cleared** and resumed waiting for Star Citizen.
- Visually confirmed that the Discord profile's Star Citizen activity was gone.
- Stopped the companion after the test; neither test process remained running.

Automated Escape input did not open the in-game pause menu, so returning from a
live session to the main menu was not verified interactively. Ship boarding,
contracts, quantum travel, and Pyro/Nyx travel were not exercised in this smoke
test. Their parser/state behavior remains covered by the documented tests and
saved-log evidence, rather than claimed as live gameplay verification.

A packaged connection check once exceeded the initial two-second handshake
timeout and passed on retry. Handshake setup now allows ten seconds; command
responses remain bounded at two seconds. A silent Windows pipe test verifies
that read deadlines are enforced rather than blocking indefinitely.

Location is event-based **last seen**, not continuous position telemetry. A game
patch can change log formats. Locations without a recognized alias use a general
status, and the last reported location expires. These limits are described in
README and are reflected in the presence wording.

## 2.1.0 content expansion

Imported the version-pinned **4.10.2-LIVE.12881860** Star Citizen Wiki API catalogue
on 14 October 2026. All 11 location pages (2,033 rows, 1,984 distinct UUIDs) and
7 vehicle pages (310 rows) were read and checked for a consistent game version
and pagination totals. Duplicate UUIDs were consolidated; placeholder names were
excluded. The embedded result has **1,953 named locations**, **18,322 aliases**,
and **310 vehicle class/variant records**. Provenance and a facts digest are in
`data/content-source.json`; the importer is `scripts/refresh-content.mjs`.

New tests cover a locally observed Cry-Astro distribution-centre inventory ID,
scoped Pyro/Nyx aliases, ambiguous gateways across systems, source counts,
vehicle entity/localization names, blueprint/refinery notices, and respawn cleanup.
The complete existing 4.10 fixture still passes. New scoped-code examples are
synthetic resolver tests and are not represented as logs observed at every POI.

The added notification shapes are documented in SC Bridge's game-log corpus.
The new vehicle and location lists are data-derived; all of them have not been
visited or flown in a live game. They can include hidden or future game records.
There is no promise of continuous detection or telemetry for unlogged mechanics.

The 2.1.0 release was also run in preview mode against the active LIVE session.
It discovered the log automatically and reconstructed the current **Anvil F7C-M
Super Hornet Mk II** presence. Preview mode did not connect to Discord or interrupt
the game. This verifies the existing live-session path with the expanded catalogue;
it does not claim a live visit to every new catalogue entry.

Deployed the verified 2.1.0 executable to the portable directory and restarted
only the existing companion, preserving its arguments. The Star Citizen process
continued running. The new companion discovered the LIVE log and successfully
published presence to Discord, with no errors in its captured error output. The
portable executable digest matches the release build; the ZIP and checksum were
refreshed.
