use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    path::Path,
};

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Location {
    pub name: String,
    pub system: String,
    pub aliases: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub scopes: Vec<String>,
}

#[derive(Debug)]
pub struct Locations {
    entries: Vec<Location>,
    aliases: HashMap<String, usize>,
    scoped: HashMap<String, Vec<(String, usize)>>,
}

impl Locations {
    pub fn load(extra: Option<&Path>) -> Result<Self> {
        let mut locations: Vec<Location> =
            serde_json::from_str(include_str!("../data/locations.json"))?;
        if let Some(path) = extra {
            let text = std::fs::read_to_string(path)
                .with_context(|| format!("Cannot read location aliases: {}", path.display()))?;
            let additional: Vec<Location> =
                serde_json::from_str(&text).context("Invalid location aliases")?;
            // User aliases override built-ins, while preserving all other built-in aliases.
            let overrides: HashSet<String> = additional
                .iter()
                .flat_map(|l| l.aliases.iter().map(|a| normalize(a)))
                .collect();
            for location in &mut locations {
                location
                    .aliases
                    .retain(|a| !overrides.contains(&normalize(a)));
            }
            locations.extend(additional);
        }
        let mut seen = HashSet::new();
        for location in &locations {
            ensure!(
                !location.name.trim().is_empty() && !location.system.trim().is_empty(),
                "Locations need a name and system"
            );
            for alias in &location.aliases {
                let normalized = normalize(alias);
                ensure!(!normalized.is_empty(), "Location aliases must not be empty");
                ensure!(seen.insert(normalized), "Duplicate location alias: {alias}");
            }
        }
        let mut aliases = HashMap::new();
        let mut scoped: HashMap<String, Vec<(String, usize)>> = HashMap::new();
        for (index, location) in locations.iter().enumerate() {
            for alias in &location.aliases {
                let alias = normalize(alias);
                aliases.insert(alias.clone(), index);
                for scope in &location.scopes {
                    let scope = normalize(scope);
                    if let Some(name) = alias.strip_prefix(&format!("{scope}_")) {
                        let name = name.replace('_', "");
                        if name.len() >= 4 {
                            scoped.entry(scope).or_default().push((name, index));
                        }
                    }
                }
            }
        }
        for candidates in scoped.values_mut() {
            candidates.sort();
            candidates.dedup();
        }
        Ok(Self {
            entries: locations,
            aliases,
            scoped,
        })
    }

    /// Exact aliases first; then most-specific hierarchical prefix with a token boundary.
    pub fn find(&self, raw: &str) -> Option<Location> {
        let normalized = normalize(raw);
        let id = normalized.strip_prefix("loc_").unwrap_or(&normalized);
        if let Some(index) = self.aliases.get(id) {
            return Some(self.entries[*index].clone());
        }
        // Only match embedded names within a source-confirmed parent scope. Tags
        // like "Settlement" and "objectContainer" may separate name components.
        let mut best: Option<(usize, usize)> = None;
        let mut ambiguous = false;
        for (scope, candidates) in &self.scoped {
            let Some(tail) = id.strip_prefix(&format!("{scope}_")) else {
                continue;
            };
            let tokens: Vec<&str> = tail.split('_').collect();
            for (name, index) in candidates {
                let found = (0..tokens.len()).any(|start| {
                    let mut part = String::new();
                    for token in &tokens[start..] {
                        part.push_str(token);
                        if part == *name {
                            return true;
                        }
                        if part.len() >= name.len() {
                            break;
                        }
                    }
                    false
                });
                if found {
                    let specificity = scope.len() + name.len();
                    match best {
                        None => {
                            best = Some((specificity, *index));
                            ambiguous = false;
                        }
                        Some((score, _)) if specificity > score => {
                            best = Some((specificity, *index));
                            ambiguous = false;
                        }
                        Some((score, previous)) if specificity == score && previous != *index => {
                            ambiguous = true;
                        }
                        _ => {}
                    }
                }
            }
        }
        if !ambiguous && let Some((_, index)) = best {
            return Some(self.entries[index].clone());
        }
        // Fall back to the most specific known parent, walking token boundaries
        // instead of allocating/normalizing every catalogue alias on every event.
        let mut parent = id;
        while let Some((prefix, _)) = parent.rsplit_once('_') {
            if let Some(index) = self.aliases.get(prefix) {
                return Some(self.entries[*index].clone());
            }
            parent = prefix;
        }
        None
    }
}

fn normalize(id: &str) -> String {
    let mut normalized = String::new();
    for c in id.chars().flat_map(char::to_lowercase) {
        if c == '\'' || c == '’' {
            continue;
        }
        if c.is_ascii_alphanumeric() {
            normalized.push(c);
        } else if !normalized.ends_with('_') {
            normalized.push('_');
        }
    }
    normalized.trim_matches('_').to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn current_catalogue_resolves_scoped_content_without_cross_system_guessing() {
        let locations = Locations::load(None).unwrap();
        assert!(locations.entries.len() > 1900);
        assert_eq!(
            locations
                .find("Stanton4_DistributionCentre_Astro_Plant34-12")
                .unwrap()
                .name,
            "Cry-Astro Processing Plant 34-12"
        );
        assert_eq!(
            locations
                .find("LOC_Pyro3_Settlement_CarversRidge_objectContainer")
                .unwrap()
                .name,
            "Carver's Ridge"
        );
        assert_eq!(
            locations
                .find("Nyx_ServiceStation_PeoplesServiceStationAlpha")
                .unwrap()
                .name,
            "People's Service Station Alpha"
        );
        assert_eq!(
            locations
                .find("8ef8417d-2d3c-4172-8030-63f63b95eed4")
                .unwrap()
                .system,
            "Nyx"
        );
        assert_eq!(
            locations.find("Pyro_StantonGateway").unwrap().system,
            "Pyro"
        );
        assert_eq!(locations.find("Nyx_StantonGateway").unwrap().system, "Nyx");
        assert!(locations.find("StantonGateway").is_none());
        assert!(
            locations
                .find("UnknownSystem_Settlement_CarversRidge")
                .is_none()
        );
        assert_ne!(
            locations
                .find("Stanton1_Settlement_CarversRidge")
                .unwrap()
                .system,
            "Pyro"
        );
    }
    #[test]
    fn specific_locations_win_and_unknown_ids_are_not_guessed() {
        let locations = Locations::load(None).unwrap();
        assert_eq!(
            locations.find("Stanton1_Lorville_Hangar").unwrap().name,
            "Lorville"
        );
        assert_eq!(locations.find("RR_HUR_LEO").unwrap().name, "Everus Harbor");
        assert_eq!(
            locations.find("LOC_RR_S2_L1").unwrap().name,
            "CRU-L1 Ambitious Dream Station"
        );
        assert_eq!(
            locations.find("Area18_City_objectContainer").unwrap().name,
            "Area18"
        );
        assert_eq!(locations.find("Pyro3").unwrap().name, "Bloom");
        assert_eq!(locations.find("Nyx_Delamar_Levski").unwrap().name, "Levski");
        assert!(locations.find("Stanton10").is_none());
        assert!(locations.find("MISSION_QT_Bounty_Beacon_1234").is_none());
    }
    #[test]
    fn custom_aliases_override_without_erasing_builtins() {
        let file = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(
            file.path(),
            r#"[{"name":"My station","system":"Pyro","aliases":["RR_HUR_LEO"]}]"#,
        )
        .unwrap();
        let locations = Locations::load(Some(file.path())).unwrap();
        assert_eq!(locations.find("RR_HUR_LEO").unwrap().name, "My station");
        assert_eq!(locations.find("Area18").unwrap().name, "Area18");
    }
}
