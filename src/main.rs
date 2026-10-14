use anyhow::{Context, Result};
use clap::Parser;
use star_citizen_rich_presence::{
    config::{Args, Config},
    discord::DiscordTransport,
    discovery::{self, GameProcess},
    locations::Locations,
    parser,
    presence::{self, Publisher, Snapshot, Transport},
    session::Session,
    tail::Tail,
};
use std::{
    io::{BufRead, BufReader},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use sysinfo::System;

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

fn main() -> Result<()> {
    let args = Args::parse();
    let config = Config::load(&args)?;
    if args.check_discord {
        let mut transport = DiscordTransport::new(&config.discord_app_id);
        let result = (|| -> Result<()> {
            transport.connect().context("Discord handshake failed")?;
            transport
                .publish(&Snapshot {
                    details: "Checking Discord integration".into(),
                    state: "Star Citizen Rich Presence".into(),
                    large_image: "logo".into(),
                    large_text: "Connection test".into(),
                    profile_url: None,
                    started: now(),
                })
                .context("Discord rejected the test presence")?;
            transport
                .clear()
                .context("Discord could not clear the test presence")?;
            println!(
                "Discord accepted the application ID and test presence. Test presence cleared."
            );
            Ok(())
        })();
        transport.close();
        return result;
    }
    let locations = Locations::load(config.locations_file.as_deref())?;
    if let Some(path) = &args.replay {
        let file =
            std::fs::File::open(path).with_context(|| format!("Cannot open {}", path.display()))?;
        let mut session = Session::new(0);
        let mut last_at = now();
        let mut events = 0;
        for line in BufReader::new(file).lines() {
            let line = line?;
            if let Some(at) = parser::timestamp(&line) {
                if session.started == 0 {
                    session.started = at;
                }
                last_at = at;
            }
            if let Some(event) = parser::parse(&line) {
                session.apply(event, last_at, &locations);
                events += 1;
            }
        }
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "events": events, "phase": session.phase,
                "presence": presence::snapshot(&session, &config, last_at),
            }))?
        );
        return Ok(());
    }
    let mut system = System::new();
    if args.discover {
        let game = discovery::running_game(&mut system, &config.channel);
        if let Some(game) = &game {
            println!("Star Citizen running (PID {})", game.pid);
            if game.executable.is_none() {
                println!(
                    "Windows did not expose its executable path; trying standard installation folders."
                );
            }
        } else {
            println!("Star Citizen is not running on {}.", config.channel);
        }
        if let Some(path) = &config.log_path {
            println!(
                "Configured log: {} (exists: {})",
                path.display(),
                path.is_file()
            );
        }
        for path in discovery::candidate_logs(&config.channel, game.as_ref())
            .iter()
            .filter(|p| p.is_file())
        {
            println!("Found: {}", path.display());
        }
        return Ok(());
    }
    let running = Arc::new(AtomicBool::new(true));
    let signal = Arc::clone(&running);
    ctrlc::set_handler(move || {
        signal.store(false, Ordering::SeqCst);
    })
    .context("Cannot install shutdown handler")?;
    println!(
        "Star Citizen Rich Presence {} · {}",
        env!("CARGO_PKG_VERSION"),
        config.channel
    );
    println!("Reading Game.log only. Press Ctrl+C to stop.");
    if args.dry_run {
        println!("Preview mode: no Discord connection.");
    }
    let mut publisher = Publisher::new(
        DiscordTransport::new(&config.discord_app_id),
        Duration::from_secs(config.update_seconds),
        Instant::now(),
    );
    let mut game_key: Option<(u32, u64)> = None;
    let mut tail: Option<Tail> = None;
    let mut session: Option<Session> = None;
    let mut last_preview: Option<Snapshot> = None;
    let mut last_warning: Option<String> = None;
    let mut waiting_logged = false;
    let mut next_discovery = Instant::now();
    while running.load(Ordering::SeqCst) {
        let game: Option<GameProcess> = discovery::running_game(&mut system, &config.channel);
        let key = game.as_ref().map(|g| (g.pid, g.started));
        if key != game_key {
            game_key = key;
            tail = None;
            session = game.as_ref().map(|g| {
                Session::new(if g.started == 0 {
                    now()
                } else {
                    g.started as i64
                })
            });
            last_warning = None;
            waiting_logged = false;
            next_discovery = Instant::now();
        }
        if let Some(game) = &game {
            if tail.is_none() && Instant::now() >= next_discovery {
                let path = config
                    .log_path
                    .clone()
                    .or_else(|| discovery::discover_log(&config.channel, Some(game)));
                if let Some(path) = path {
                    println!("Log: {}", path.display());
                    tail = Some(Tail::new(path));
                } else {
                    warn_once(&mut last_warning, "Game found, but no log located. Use --game-dir or --log for a custom installation.".into());
                }
                next_discovery = Instant::now() + Duration::from_secs(10);
            }
            if let (Some(tail), Some(session)) = (&mut tail, &mut session) {
                match tail.poll() {
                    Ok(batch) => {
                        if batch.reset {
                            *session = Session::new(if game.started == 0 {
                                now()
                            } else {
                                game.started as i64
                            });
                        }
                        for line in batch.lines {
                            let at = parser::timestamp(&line).unwrap_or_else(now);
                            // Ignore the previous launch's log while the new process is creating its log.
                            if game.started != 0 && at < session.started.saturating_sub(5) {
                                continue;
                            }
                            if let Some(event) = parser::parse(&line) {
                                session.apply(event, at, &locations);
                            }
                        }
                        last_warning = None;
                    }
                    Err(error) => warn_once(
                        &mut last_warning,
                        format!(
                            "Waiting for readable log {}: {error}",
                            tail.path().display()
                        ),
                    ),
                }
            }
        } else if !waiting_logged {
            println!("Waiting for Star Citizen...");
            waiting_logged = true;
        }
        let desired = session
            .as_ref()
            .and_then(|s| presence::snapshot(s, &config, now()));
        if args.dry_run {
            if desired != last_preview {
                println!("{}", serde_json::to_string_pretty(&desired)?);
                last_preview = desired;
            }
        } else {
            match publisher.sync(desired.as_ref(), Instant::now()) {
                Ok(true) => println!(
                    "Presence: {}",
                    desired.as_ref().map_or("cleared", |s| s.details.as_str())
                ),
                Ok(false) => {}
                Err(error) => warn_once(
                    &mut last_warning,
                    format!("Discord unavailable; will reconnect: {error}"),
                ),
            }
        }
        // Short waits make Ctrl+C responsive even with a long polling interval.
        let deadline = Instant::now() + Duration::from_secs(config.poll_seconds);
        while running.load(Ordering::SeqCst) && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(100));
        }
    }
    publisher.shutdown();
    println!("Presence cleared. Goodbye.");
    Ok(())
}

fn warn_once(last: &mut Option<String>, message: String) {
    if last.as_ref() != Some(&message) {
        eprintln!("{message}");
        *last = Some(message);
    }
}
