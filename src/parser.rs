use chrono::DateTime;
use regex::Regex;
use std::sync::OnceLock;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    Login(String),
    Mode(String),
    Loading,
    Ready,
    Disconnect,
    Shutdown,
    Inventory {
        player: String,
        location: String,
    },
    QuantumTarget {
        vehicle: String,
        destination: String,
    },
    QuantumArrival {
        vehicle: String,
    },
    Board(String),
    Disembark(String),
    ContractAccepted(String),
    ContractEnded(String),
    Incapacitated,
    Spawned,
    Notice {
        details: String,
        state: String,
    },
}

struct Patterns {
    login: Regex,
    mode: Regex,
    inventory: Regex,
    target: Regex,
    vehicle: Regex,
    notification: Regex,
}

fn patterns() -> &'static Patterns {
    static PATTERNS: OnceLock<Patterns> = OnceLock::new();
    PATTERNS.get_or_init(|| Patterns {
        login: Regex::new(r"User Login Success - Handle\[([^\]]+)\]").unwrap(),
        mode: Regex::new(r"GameMode\[([^\]]+)\]").unwrap(),
        inventory: Regex::new(r"Player\[([^\]]+)\] requested inventory for Location\[([^\]]+)\]")
            .unwrap(),
        target: Regex::new(r"Player has selected point (\S+) as their destination").unwrap(),
        vehicle: Regex::new(r"([A-Za-z0-9_]+\[\d+\])\|CSCItemNavigation::").unwrap(),
        // Some HUD notifications continue on the next line. Their first line is enough here.
        notification: Regex::new(r#"Added notification "([^"\r\n]*)"#).unwrap(),
    })
}

pub fn timestamp(line: &str) -> Option<i64> {
    let time = line.strip_prefix('<')?.split_once('>')?.0;
    DateTime::parse_from_rfc3339(time)
        .ok()
        .map(|t| t.timestamp())
}

/// Only explicit client events are interpreted. Bare mentions of planets/ships are ignored.
pub fn parse(line: &str) -> Option<Event> {
    let p = patterns();
    if let Some(m) = p.login.captures(line) {
        let handle = m[1].trim();
        if !handle.is_empty() {
            return Some(Event::Login(handle.into()));
        }
    }
    if line.contains("System Fast Shutdown") || line.contains("[CIG] CCIGBroker::FastShutdown") {
        return Some(Event::Shutdown);
    }
    if line.contains("[CSessionManager::OnClientSpawned] Spawned!") {
        return Some(Event::Spawned);
    }
    if line.contains("<Channel Disconnected>") && line.contains("hostType=\"GameClient\"") {
        // Local frontend teardown happens while JOINING a server, not returning to menu.
        if line.contains("gamerules=\"SC_Frontend\"") {
            return Some(Event::Loading);
        }
        return Some(Event::Disconnect);
    }
    if line.contains("SeedGameRulesAndMode Success>") {
        return p.mode.captures(line).map(|m| Event::Mode(m[1].into()));
    }
    if line.contains("Loading level ") {
        return Some(Event::Loading);
    }
    if line.contains("<[GameRules] GameRulesActionEvent_GameModeCreated>") {
        return Some(Event::Ready);
    }
    if line.contains("<RequestLocationInventory>") {
        return p.inventory.captures(line).map(|m| Event::Inventory {
            player: m[1].into(),
            location: m[2].into(),
        });
    }
    if line.contains("<Player Selected Quantum Target - Local>") {
        let vehicle = p.vehicle.captures(line)?;
        let target = p.target.captures(line)?;
        return Some(Event::QuantumTarget {
            vehicle: vehicle[1].into(),
            destination: target[1].into(),
        });
    }
    if line.contains("<Quantum Drive Arrived - Arrived at Final Destination>") {
        return p.vehicle.captures(line).map(|m| Event::QuantumArrival {
            vehicle: m[1].into(),
        });
    }
    if !line.contains("<SHUDEvent_OnNotification>") {
        return None;
    }
    let message = p.notification.captures(line)?;
    let message = message[1].trim();
    if let Some(channel) = message
        .strip_prefix("You have joined channel '")
        .and_then(|s| s.split_once('\'').map(|(s, _)| s))
    {
        return ship_name(channel).map(Event::Board);
    }
    if let Some(channel) = message
        .strip_prefix("You have left the channel '")
        .and_then(|s| s.split_once('\'').map(|(s, _)| s))
    {
        return ship_name(channel).map(Event::Disembark);
    }
    if let Some(name) = message.strip_prefix("Contract Accepted:") {
        return nonempty_contract(name).map(Event::ContractAccepted);
    }
    for prefix in [
        "Contract Complete:",
        "Contract Failed:",
        "Contract Abandoned:",
    ] {
        if let Some(name) = message.strip_prefix(prefix) {
            return nonempty_contract(name).map(Event::ContractEnded);
        }
    }
    if message.starts_with("Incapacitated:") {
        return Some(Event::Incapacitated);
    }
    if let Some(name) = message
        .strip_prefix("Received Blueprint:")
        .and_then(nonempty_contract)
    {
        return Some(Event::Notice {
            details: "Blueprint acquired".into(),
            state: name,
        });
    }
    if let Some(location) = message
        .strip_prefix("A Refinery Work Order has been Completed at ")
        .and_then(nonempty_contract)
    {
        // This can be a remote refinery. Never treat it as the player's current location.
        return Some(Event::Notice {
            details: "Refinery order completed".into(),
            state: location,
        });
    }
    if message.starts_with("Hangar Request Completed:") {
        return Some(Event::Notice {
            details: "Hangar ready".into(),
            state: String::new(),
        });
    }
    None
}

fn ship_name(channel: &str) -> Option<String> {
    // Vehicle channels include an owner. Party/global channel joins must not become ships.
    let (ship, _) = channel.rsplit_once(" : ")?;
    let ship = ship
        .trim()
        .strip_prefix("@vehicle_Name")
        .unwrap_or(ship.trim());
    (!ship.is_empty()).then(|| crate::vehicles::display_name(ship))
}

fn nonempty_contract(name: &str) -> Option<String> {
    let name = name.trim().trim_end_matches(':').trim();
    (!name.is_empty()).then(|| name.into())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn newer_vehicle_names_and_local_notices_are_parsed_precisely() {
        assert_eq!(
            parse(
                "<SHUDEvent_OnNotification> Added notification \"You have joined channel '@vehicle_NameRSI_Apollo_Medivac : TestPilot'."
            ),
            Some(Event::Board("RSI Apollo Medivac".into()))
        );
        assert_eq!(
            parse(
                "<SHUDEvent_OnNotification> Added notification \"Received Blueprint: Geist Armor Core Desert: "
            ),
            Some(Event::Notice {
                details: "Blueprint acquired".into(),
                state: "Geist Armor Core Desert".into()
            })
        );
        assert_eq!(
            parse(
                "<SHUDEvent_OnNotification> Added notification \"A Refinery Work Order has been Completed at CRU-L1 Ambitious Dream Station: "
            ),
            Some(Event::Notice {
                details: "Refinery order completed".into(),
                state: "CRU-L1 Ambitious Dream Station".into()
            })
        );
        assert_eq!(
            parse("[CSessionManager::OnClientSpawned] Spawned!"),
            Some(Event::Spawned)
        );
        assert_eq!(
            parse("A mission description about Received Blueprint: Mining"),
            None
        );
    }
    #[test]
    fn ignores_world_noise_and_remote_clients() {
        assert_eq!(parse("Streaming entity Stanton3_Area18"), None);
        assert_eq!(parse("<Channel Disconnected> hostType=\"Replicant\""), None);
        assert_eq!(
            parse(
                "<SHUDEvent_OnNotification> Added notification \"You have joined channel 'Global'.\""
            ),
            None
        );
        assert_eq!(parse("<RequestLocationInventory> malformed"), None);
    }
    #[test]
    fn parses_real_410_shapes_with_redacted_identity() {
        assert_eq!(
            parse(
                "<RequestLocationInventory> Player[TestPilot] requested inventory for Location[RR_HUR_LEO] [Team_CoreGameplayFeatures][Inventory]"
            ),
            Some(Event::Inventory {
                player: "TestPilot".into(),
                location: "RR_HUR_LEO".into()
            })
        );
        assert_eq!(
            parse(
                "<SHUDEvent_OnNotification> Added notification \"Contract Accepted:  Combat Gauntlet - Scenario #5: "
            ),
            Some(Event::ContractAccepted(
                "Combat Gauntlet - Scenario #5".into()
            ))
        );
        assert_eq!(
            parse(
                "<SHUDEvent_OnNotification> Added notification \"You have joined channel 'Anvil F7C-M Super Hornet Mk II : TestPilot'."
            ),
            Some(Event::Board("Anvil F7C-M Super Hornet Mk II".into()))
        );
        assert_eq!(
            parse("<Channel Disconnected> gamerules=\"SC_Frontend\" hostType=\"GameClient\""),
            Some(Event::Loading)
        );
    }
    #[test]
    fn timestamps_are_optional_and_checked() {
        assert_eq!(
            timestamp("<2026-10-14T16:17:22.616Z> FileVersion: 4.10.197.2878"),
            Some(1791994642)
        );
        assert_eq!(timestamp("<not-a-date>"), None);
    }
}
