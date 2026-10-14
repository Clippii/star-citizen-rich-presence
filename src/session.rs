use crate::{
    locations::{Location, Locations},
    parser::Event,
};
use serde::Serialize;
use std::collections::BTreeSet;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum Phase {
    Starting,
    Menu,
    Loading,
    Playing,
    Stopped,
}

#[derive(Debug)]
pub struct Session {
    pub started: i64,
    pub phase: Phase,
    pub handle: Option<String>,
    pub ship: Option<String>,
    pub contracts: BTreeSet<String>,
    pub location: Option<(Location, i64)>,
    pub incapacitated_at: Option<i64>,
    pub notice: Option<(String, String, i64)>,
    target: Option<(String, String, i64)>,
}

impl Session {
    pub fn new(started: i64) -> Self {
        Self {
            started,
            phase: Phase::Starting,
            handle: None,
            ship: None,
            contracts: BTreeSet::new(),
            location: None,
            incapacitated_at: None,
            notice: None,
            target: None,
        }
    }

    fn clear_gameplay(&mut self) {
        self.ship = None;
        self.contracts.clear();
        self.location = None;
        self.target = None;
        self.incapacitated_at = None;
        self.notice = None;
    }

    pub fn apply(&mut self, event: Event, at: i64, locations: &Locations) {
        match event {
            Event::Login(handle) => {
                if self.handle.as_ref().is_some_and(|old| old != &handle) {
                    self.clear_gameplay();
                }
                self.handle = Some(handle);
            }
            Event::Mode(mode) => {
                self.clear_gameplay();
                self.phase = if mode == "SC_Frontend" {
                    Phase::Menu
                } else {
                    Phase::Playing
                };
            }
            Event::Loading => {
                self.clear_gameplay();
                self.phase = Phase::Loading;
            }
            Event::Ready => {
                if self.phase != Phase::Menu {
                    self.phase = Phase::Playing;
                }
            }
            Event::Disconnect => {
                self.clear_gameplay();
                self.phase = Phase::Menu;
            }
            Event::Shutdown => {
                self.clear_gameplay();
                self.phase = Phase::Stopped;
            }
            Event::Inventory { player, location } => {
                // This log includes other actors too. Require the authenticated local handle.
                if self
                    .handle
                    .as_ref()
                    .is_some_and(|h| h.eq_ignore_ascii_case(&player))
                {
                    self.phase = Phase::Playing;
                    self.location = locations.find(&location).map(|l| (l, at));
                    self.incapacitated_at = None;
                }
            }
            Event::QuantumTarget {
                vehicle,
                destination,
            } => {
                self.phase = Phase::Playing;
                if let Some(ship) = crate::vehicles::find(&vehicle) {
                    self.ship = Some(ship);
                }
                self.target = Some((vehicle, destination, at));
            }
            Event::QuantumArrival { vehicle } => {
                self.location = None;
                if let Some((target_vehicle, target, selected)) = self.target.take() {
                    if target_vehicle == vehicle && (0..=600).contains(&(at - selected)) {
                        self.phase = Phase::Playing;
                        // Unknown/mission destinations invalidate the previous location.
                        self.location = locations.find(&target).map(|l| (l, at));
                    }
                } else {
                    // Without a selected target we know travel occurred, but not where.
                    self.location = None;
                }
            }
            Event::Board(ship) => {
                self.phase = Phase::Playing;
                self.ship = Some(ship);
                self.incapacitated_at = None;
            }
            Event::Disembark(ship) => {
                if self.ship.as_ref() == Some(&ship) {
                    self.ship = None;
                    self.target = None;
                }
            }
            Event::ContractAccepted(contract) => {
                self.phase = Phase::Playing;
                if self.contracts.len() < 32 {
                    self.contracts.insert(contract);
                }
            }
            Event::ContractEnded(contract) => {
                self.contracts.remove(&contract);
            }
            Event::Incapacitated => {
                self.incapacitated_at = Some(at);
                self.location = None;
                self.target = None;
                self.notice = None;
            }
            Event::Spawned => {
                if matches!(self.phase, Phase::Menu | Phase::Stopped) {
                    return;
                }
                self.phase = Phase::Playing;
                self.ship = None;
                self.location = None;
                self.target = None;
                self.incapacitated_at = None;
                self.notice = None;
            }
            Event::Notice { details, state } => {
                if self.phase == Phase::Playing {
                    self.notice = Some((details, state, at));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn locations() -> Locations {
        Locations::load(None).unwrap()
    }
    #[test]
    fn respawn_clears_stale_vehicle_and_location_but_keeps_active_contracts() {
        let locations = locations();
        let mut s = Session::new(123);
        s.apply(Event::Login("Pilot".into()), 124, &locations);
        s.apply(
            Event::Inventory {
                player: "Pilot".into(),
                location: "Area18".into(),
            },
            125,
            &locations,
        );
        s.apply(Event::Board("RSI Apollo Medivac".into()), 126, &locations);
        s.apply(
            Event::ContractAccepted("New Alliance Aid contract".into()),
            127,
            &locations,
        );
        s.apply(Event::Incapacitated, 128, &locations);
        s.apply(Event::Spawned, 129, &locations);
        assert_eq!(s.phase, Phase::Playing);
        assert!(s.ship.is_none() && s.location.is_none() && s.incapacitated_at.is_none());
        assert_eq!(s.contracts.len(), 1);
        assert_eq!(s.started, 123);
    }
    #[test]
    fn inventory_is_attributed_and_unknown_locations_clear_stale_state() {
        let mut s = Session::new(0);
        s.apply(Event::Login("Pilot".into()), 1, &locations());
        s.apply(
            Event::Inventory {
                player: "Other".into(),
                location: "Area18".into(),
            },
            2,
            &locations(),
        );
        assert!(s.location.is_none());
        s.apply(
            Event::Inventory {
                player: "Pilot".into(),
                location: "Area18".into(),
            },
            3,
            &locations(),
        );
        assert_eq!(s.location.as_ref().unwrap().0.name, "Area18");
        s.apply(
            Event::Inventory {
                player: "Pilot".into(),
                location: "NewUnmappedArea".into(),
            },
            4,
            &locations(),
        );
        assert!(s.location.is_none());
    }
    #[test]
    fn selection_is_not_arrival_and_arrivals_require_the_same_vehicle() {
        let mut s = Session::new(0);
        s.apply(
            Event::QuantumTarget {
                vehicle: "ship1".into(),
                destination: "Area18".into(),
            },
            1,
            &locations(),
        );
        assert!(s.location.is_none());
        s.apply(
            Event::QuantumArrival {
                vehicle: "ship2".into(),
            },
            2,
            &locations(),
        );
        assert!(s.location.is_none());
        s.apply(
            Event::QuantumTarget {
                vehicle: "ship1".into(),
                destination: "Area18".into(),
            },
            3,
            &locations(),
        );
        s.apply(
            Event::QuantumArrival {
                vehicle: "ship1".into(),
            },
            4,
            &locations(),
        );
        assert_eq!(s.location.as_ref().unwrap().0.name, "Area18");
        s.apply(
            Event::QuantumTarget {
                vehicle: "ship1".into(),
                destination: "MISSION_QT_Bounty_Beacon_123".into(),
            },
            5,
            &locations(),
        );
        s.apply(
            Event::QuantumArrival {
                vehicle: "ship1".into(),
            },
            6,
            &locations(),
        );
        assert!(s.location.is_none());
    }
    #[test]
    fn teardown_clears_gameplay_and_preserves_session_time() {
        let mut s = Session::new(123);
        s.apply(Event::Board("Ship".into()), 124, &locations());
        s.apply(Event::ContractAccepted("Haul".into()), 125, &locations());
        s.apply(Event::Disconnect, 126, &locations());
        assert_eq!(s.started, 123);
        assert_eq!(s.phase, Phase::Menu);
        assert!(s.ship.is_none() && s.contracts.is_empty());
    }
}
