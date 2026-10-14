//! Current versioned game vehicle names, embedded for offline use.
use serde::Deserialize;
use std::{collections::HashMap, sync::OnceLock};

#[derive(Deserialize)]
struct Vehicle {
    name: String,
    class_name: String,
}

fn key(raw: &str) -> String {
    raw.chars()
        .filter(char::is_ascii_alphanumeric)
        .flat_map(char::to_lowercase)
        .collect()
}

fn names() -> &'static HashMap<String, String> {
    static NAMES: OnceLock<HashMap<String, String>> = OnceLock::new();
    NAMES.get_or_init(|| {
        let vehicles: Vec<Vehicle> = serde_json::from_str(include_str!("../data/vehicles.json"))
            .expect("Embedded vehicle catalogue is valid");
        let mut names = HashMap::new();
        for vehicle in vehicles {
            names.insert(key(&vehicle.class_name), vehicle.name.clone());
            names.insert(key(&vehicle.name), vehicle.name);
        }
        names
    })
}

pub fn find(raw: &str) -> Option<String> {
    let raw = raw
        .trim()
        .strip_prefix("@vehicle_Name")
        .unwrap_or(raw.trim());
    // Runtime entities append the same instance ID before and inside brackets.
    let class = if let Some((class, instance)) = raw.rsplit_once('[') {
        let instance = instance.strip_suffix(']')?;
        if instance.is_empty() || !instance.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        class.strip_suffix(&format!("_{instance}")).unwrap_or(class)
    } else {
        raw
    };
    names().get(&key(class)).cloned()
}

pub fn display_name(raw: &str) -> String {
    find(raw).unwrap_or_else(|| {
        raw.trim()
            .strip_prefix("@vehicle_Name")
            .unwrap_or(raw.trim())
            .replace('_', " ")
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn resolves_current_variants_and_keeps_unknown_ships_readable() {
        assert_eq!(
            find("ANVL_Hornet_F7CM_Mk2_880166317121[880166317121]").as_deref(),
            Some("Anvil F7C-M Super Hornet Mk II")
        );
        assert_eq!(
            find("@vehicle_NameRSI_Apollo_Medivac").as_deref(),
            Some("RSI Apollo Medivac")
        );
        assert_eq!(
            find("DRAK_Golem_Collector_Indust").as_deref(),
            Some("Drake Golem Wikelo Work Special")
        );
        assert_eq!(display_name("Unmapped_New_Ship"), "Unmapped New Ship");
        assert!(find("RSI_Apollo_Medivac[not-an-id]").is_none());
    }
}
