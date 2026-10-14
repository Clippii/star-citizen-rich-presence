use star_citizen_rich_presence::{
    config::Config,
    locations::Locations,
    parser, presence,
    session::{Phase, Session},
};

#[test]
fn complete_410_session_tracks_local_events_without_overlay() {
    let locations = Locations::load(None).unwrap();
    let mut session = Session::new(1791993600);
    let mut at = session.started;
    for line in include_str!("fixtures/session-4.10.log").lines() {
        at = parser::timestamp(line).unwrap_or(at);
        if let Some(event) = parser::parse(line) {
            session.apply(event, at, &locations);
        }
    }
    assert_eq!(session.phase, Phase::Playing);
    assert!(session.contracts.is_empty());
    assert!(session.ship.is_none());
    let result = presence::snapshot(&session, &Config::default(), at).unwrap();
    assert_eq!(result.details, "Last seen: Everus Harbor");
    assert_eq!(result.state, "Stanton · LIVE");
    assert_eq!(
        result.profile_url.as_deref(),
        Some("https://robertsspaceindustries.com/citizens/TestPilot")
    );
    assert_eq!(result.started, 1791993600);
}

#[test]
fn executable_replay_prints_json_and_respects_privacy() {
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_star-citizen-rich-presence"))
        .args([
            "--config",
            "scrp.example.toml",
            "--replay",
            "tests/fixtures/session-4.10.log",
            "--no-profile",
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let result: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(result["phase"], "Playing");
    assert_eq!(result["presence"]["details"], "Last seen: Everus Harbor");
    assert!(result["presence"]["profile_url"].is_null());
}
