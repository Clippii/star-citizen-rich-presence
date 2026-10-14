use crate::{
    config::Config,
    session::{Phase, Session},
};
use anyhow::Result;
use serde::Serialize;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Snapshot {
    pub details: String,
    pub state: String,
    pub large_image: String,
    pub large_text: String,
    pub profile_url: Option<String>,
    pub started: i64,
}

pub fn snapshot(session: &Session, config: &Config, now: i64) -> Option<Snapshot> {
    let (details, mut state) = match session.phase {
        Phase::Stopped => return None,
        Phase::Starting => ("Starting Star Citizen".into(), config.channel.clone()),
        Phase::Menu => ("In the main menu".into(), config.channel.clone()),
        Phase::Loading => ("Loading into the verse".into(), config.channel.clone()),
        Phase::Playing => ("Exploring the verse".into(), config.channel.clone()),
    };
    let mut snapshot = Snapshot {
        details,
        state: String::new(),
        large_image: "logo".into(),
        large_text: "Star Citizen".into(),
        profile_url: None,
        started: session.started,
    };
    if session.phase == Phase::Playing {
        if let Some((location, at)) = &session.location
            && (0..=config.location_ttl_seconds as i64).contains(&(now - at))
        {
            snapshot.details = format!("Last seen: {}", location.name);
            state = format!("{} · {}", location.system, config.channel);
            snapshot.large_text = location.name.clone();
            if let Some(asset) = config.image_assets.get(&location.name) {
                snapshot.large_image = asset.clone();
            }
        }
        if let Some(ship) = &session.ship {
            state = ship.clone();
        }
        if config.show_contracts {
            if session.contracts.len() == 1 {
                state = session.contracts.first().unwrap().clone();
            } else if session.contracts.len() > 1 {
                state = format!("{} active contracts", session.contracts.len());
            }
        }
        if config.show_notifications
            && let Some((details, secondary, at)) = &session.notice
            && (0..=60).contains(&(now - at))
        {
            snapshot.details = details.clone();
            if !secondary.is_empty() {
                state = secondary.clone();
            }
        }
        if session
            .incapacitated_at
            .is_some_and(|at| (0..=120).contains(&(now - at)))
        {
            snapshot.details = "Incapacitated".into();
        }
    }
    if config.show_profile
        && let Some(handle) = &session.handle
        && !handle.is_empty()
        && handle.len() <= 64
        && handle
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
    {
        snapshot.profile_url = Some(format!(
            "https://robertsspaceindustries.com/citizens/{handle}"
        ));
    }
    snapshot.details = limit(&snapshot.details, 128);
    snapshot.state = limit(&state, 128);
    snapshot.large_text = limit(&snapshot.large_text, 128);
    Some(snapshot)
}

fn limit(text: &str, max: usize) -> String {
    if text.len() <= max {
        return text.into();
    }
    let mut end = max - 3;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}...", &text[..end])
}

pub trait Transport {
    fn connect(&mut self) -> Result<()>;
    fn publish(&mut self, snapshot: &Snapshot) -> Result<()>;
    fn clear(&mut self) -> Result<()>;
    fn close(&mut self);
}

/// Remember only successful sends. Reconnects always republish, even if state is unchanged.
pub struct Publisher<T: Transport> {
    transport: T,
    connected: bool,
    last: Option<Snapshot>,
    sent_at: Option<Instant>,
    retry_at: Instant,
    interval: Duration,
}

impl<T: Transport> Publisher<T> {
    pub fn new(transport: T, interval: Duration, now: Instant) -> Self {
        Self {
            transport,
            connected: false,
            last: None,
            sent_at: None,
            retry_at: now,
            interval,
        }
    }

    pub fn sync(&mut self, desired: Option<&Snapshot>, now: Instant) -> Result<bool> {
        if desired.is_none() {
            if self.connected {
                let result = self.transport.clear();
                self.transport.close();
                self.connected = false;
                self.last = None;
                self.sent_at = None;
                result?;
                return Ok(true);
            }
            return Ok(false);
        }
        if now < self.retry_at {
            return Ok(false);
        }
        if let Some(sent) = self.sent_at {
            if now.duration_since(sent) < self.interval {
                return Ok(false);
            }
            // Once a minute, refresh to detect a closed/restarted Discord pipe.
            if desired == self.last.as_ref() && now.duration_since(sent) < Duration::from_secs(60) {
                return Ok(false);
            }
        }
        let result = (|| {
            if !self.connected {
                self.transport.connect()?;
                self.connected = true;
            }
            self.transport.publish(desired.unwrap())
        })();
        if let Err(error) = result {
            self.transport.close();
            self.connected = false;
            self.last = None;
            self.sent_at = None;
            self.retry_at = now + self.interval;
            return Err(error);
        }
        self.last = desired.cloned();
        self.sent_at = Some(now);
        Ok(true)
    }

    pub fn shutdown(&mut self) {
        if self.connected {
            let _ = self.transport.clear();
            self.transport.close();
        }
        self.connected = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{locations::Locations, parser::Event};
    #[test]
    fn remote_refinery_and_blueprint_notices_expire_without_moving_the_player() {
        let locations = Locations::load(None).unwrap();
        let mut s = Session::new(1);
        s.apply(Event::Login("Pilot".into()), 2, &locations);
        s.apply(
            Event::Inventory {
                player: "Pilot".into(),
                location: "Stanton1_Lorville".into(),
            },
            3,
            &locations,
        );
        s.apply(
            Event::Notice {
                details: "Refinery order completed".into(),
                state: "CRU-L1 Ambitious Dream Station".into(),
            },
            4,
            &locations,
        );
        let fresh = snapshot(&s, &Config::default(), 5).unwrap();
        assert_eq!(fresh.details, "Refinery order completed");
        assert_eq!(s.location.as_ref().unwrap().0.name, "Lorville");
        assert_eq!(
            snapshot(&s, &Config::default(), 65).unwrap().details,
            "Last seen: Lorville"
        );
        let hidden = Config {
            show_notifications: false,
            ..Config::default()
        };
        assert_eq!(
            snapshot(&s, &hidden, 5).unwrap().details,
            "Last seen: Lorville"
        );
    }
    use anyhow::anyhow;
    #[derive(Default)]
    struct Fake {
        connects: usize,
        sends: usize,
        clears: usize,
        fail: bool,
    }
    impl Transport for Fake {
        fn connect(&mut self) -> Result<()> {
            self.connects += 1;
            Ok(())
        }
        fn publish(&mut self, _: &Snapshot) -> Result<()> {
            self.sends += 1;
            if self.fail {
                Err(anyhow!("pipe closed"))
            } else {
                Ok(())
            }
        }
        fn clear(&mut self) -> Result<()> {
            self.clears += 1;
            Ok(())
        }
        fn close(&mut self) {}
    }
    #[test]
    fn deduplicates_rate_limits_and_republishes_after_failure() {
        let now = Instant::now();
        let mut p = Publisher::new(Fake::default(), Duration::from_secs(15), now);
        let s = snapshot(&Session::new(1), &Config::default(), 1).unwrap();
        assert!(!p.sync(None, now).unwrap());
        assert!(p.sync(Some(&s), now).unwrap());
        assert!(!p.sync(Some(&s), now + Duration::from_secs(20)).unwrap());
        p.transport.fail = true;
        assert!(p.sync(Some(&s), now + Duration::from_secs(60)).is_err());
        p.transport.fail = false;
        assert!(!p.sync(Some(&s), now + Duration::from_secs(65)).unwrap());
        assert!(p.sync(Some(&s), now + Duration::from_secs(75)).unwrap());
        assert_eq!(p.transport.connects, 2);
        assert!(p.sync(None, now + Duration::from_secs(76)).unwrap());
        assert_eq!(p.transport.clears, 1);
    }
    #[test]
    fn stale_location_and_profile_opt_out_work() {
        let mut s = Session::new(1);
        s.apply(
            Event::Login("Pilot".into()),
            2,
            &Locations::load(None).unwrap(),
        );
        s.apply(
            Event::Inventory {
                player: "Pilot".into(),
                location: "Area18".into(),
            },
            3,
            &Locations::load(None).unwrap(),
        );
        assert_eq!(
            snapshot(&s, &Config::default(), 4).unwrap().details,
            "Last seen: Area18"
        );
        let config = Config {
            show_profile: false,
            ..Config::default()
        };
        let later = snapshot(&s, &config, 1000).unwrap();
        assert_eq!(later.details, "Exploring the verse");
        assert!(later.profile_url.is_none());
        assert!(limit(&"é".repeat(100), 128).len() <= 128);
    }
}
