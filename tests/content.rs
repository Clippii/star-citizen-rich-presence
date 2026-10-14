use star_citizen_rich_presence::{
    config::Config, locations::Locations, parser, presence, session::Session,
};

#[test]
fn modern_poi_and_vehicle_identifiers_reach_the_discord_payload() {
    let locations = Locations::load(None).unwrap();
    let mut session = Session::new(1);
    for (at, line) in [
        "[CIG-net] User Login Success - Handle[TestPilot]",
        "[CSessionManager::OnClientSpawned] Spawned!",
        "<RequestLocationInventory> Player[TestPilot] requested inventory for Location[Stanton4_DistributionCentre_Astro_Plant34-12]",
        "<SHUDEvent_OnNotification> Added notification \"You have joined channel '@vehicle_NameRSI_Apollo_Medivac : TestPilot'.",
    ].iter().enumerate() {
        session.apply(parser::parse(line).unwrap(), at as i64 + 2, &locations);
    }
    let payload = presence::snapshot(&session, &Config::default(), 6).unwrap();
    assert_eq!(
        payload.details,
        "Last seen: Cry-Astro Processing Plant 34-12"
    );
    assert_eq!(payload.state, "RSI Apollo Medivac");
    session.apply(parser::parse("<SHUDEvent_OnNotification> Added notification \"Received Blueprint: New Research Equipment: ").unwrap(), 7, &locations);
    let payload = presence::snapshot(&session, &Config::default(), 8).unwrap();
    assert_eq!(payload.details, "Blueprint acquired");
    assert_eq!(payload.state, "New Research Equipment");
    assert_eq!(
        presence::snapshot(&session, &Config::default(), 68)
            .unwrap()
            .state,
        "RSI Apollo Medivac"
    );
}

#[test]
fn source_metadata_matches_bundled_catalogue_counts_and_version() {
    let provenance: serde_json::Value =
        serde_json::from_str(include_str!("../data/content-source.json")).unwrap();
    let locations: Vec<serde_json::Value> =
        serde_json::from_str(include_str!("../data/locations.json")).unwrap();
    let vehicles: Vec<serde_json::Value> =
        serde_json::from_str(include_str!("../data/vehicles.json")).unwrap();
    assert_eq!(
        provenance["catalogued_locations"].as_u64().unwrap() as usize,
        locations.len()
    );
    assert_eq!(
        provenance["catalogued_vehicles"].as_u64().unwrap() as usize,
        vehicles.len()
    );
    assert_eq!(provenance["game_version"], "4.10.2-LIVE.12881860");
}
