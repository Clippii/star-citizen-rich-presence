# 2.1.0 — Current game-content update

- Replaces the initial 65-entry catalogue with 1,953 named location records from
  the versioned 4.10.2 game-data snapshot: Stanton, Pyro, and Nyx.
- Includes stations, outposts, distribution centres, caves, contested-zone records,
  Nyx service/QV station records, and other named starmap points.
- Adds 310 vehicle class/variant names, including current Mk, Alliance, and Wikelo
  variants. Friendly names work for localized vehicle-channel and runtime entity IDs.
- Matches names inside a confirmed parent scope before falling back to the planet.
  Duplicate unqualified destinations and shared hierarchy tags are not guessed.
- Adds short-lived blueprint-acquired, refinery-completed, and hangar-ready notices.
  Disable them with `--no-notifications` or `show_notifications = false`.
- Clears stale vehicle/location state on respawn while preserving session time.
- Retains custom aliases, privacy flags, offline operation, and the original Discord
  application ID. No OCR, overlay, memory access, or game configuration edits.
- Adds a reproducible developer catalogue refresh tool and source provenance.

The lists contain named extracted-game records, not a claim that every point is
currently visitable or every variant player-flyable. Detection still depends on
events the game logs. New mechanics with no relevant log event are not fabricated.

Validated with 29 automated tests, formatting, linting, release build, and the
existing 4.10 session replay. See VALIDATION.md for the live-test scope.
