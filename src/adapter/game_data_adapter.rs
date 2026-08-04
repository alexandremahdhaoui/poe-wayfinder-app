//! Loading the stat and item tables from ndjson.
//!
//! Implements the `StatLookup` and `ItemLookup` traits `poe-trader-core`
//! declares. The domain states what it needs and this supplies it, which is
//! why the parser is testable with no file on disk.
//!
//! # Why ndjson
//!
//! One record per line. A diff shows which stat changed rather than that the
//! file changed. A 3.6 MB JSON array shows the second thing.
//!
//! # Why every lookup is a hash map
//!
//! The stat table holds about 8000 stats and the matcher tries up to 17
//! templates per stat line. A linear scan would be 136000 string compares for
//! one modifier, and an item can carry a dozen.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use poe_trader_core::adapter::data_adapter::{ItemLookup, Namespace, StatLookup};
use poe_trader_core::types::item::BaseInfo;
use poe_trader_core::types::stat::{Stat, StatBetter, StatHit, StatMatcher, TradeInfo};
use poe_trader_core::types::GameVersion;
use poe_trader_core::types::ItemCategory;
use serde::Deserialize;
use thiserror::Error;

/// Why the tables could not be loaded.
#[derive(Debug, Error)]
pub enum LoadError {
    #[error("reading {path}")]
    Read {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    /// One line failed to parse.
    ///
    /// The line number is carried because a 20000 line file with one bad
    /// record is otherwise impossible to fix.
    #[error("parsing {path} line {line}")]
    Parse {
        path: PathBuf,
        line: usize,
        #[source]
        source: serde_json::Error,
    },
}

// ---------------------------------------------------------------------------
// Wire shapes
//
// These mirror the ndjson exactly and are mapped to the domain types at the
// boundary. Letting the file's shape into the domain would make every future
// data format change a change to the parser.
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct WireMatcher {
    string: String,
    #[serde(default)]
    advanced: Option<String>,
    #[serde(default)]
    negate: bool,
    #[serde(default)]
    value: Option<f64>,
}

#[derive(Debug, Default, Deserialize)]
struct WireTrade {
    #[serde(default)]
    inverted: bool,
    #[serde(default)]
    option: bool,
    #[serde(default)]
    count: bool,
    /// Optional AND nullable.
    ///
    /// The reference writes `"ids": null` for stats that exist in the game but
    /// have no trade filter, such as a logbook faction name. A plain
    /// `#[serde(default)]` handles a missing key and rejects an explicit null,
    /// so the real data file fails to load without this.
    #[serde(default)]
    ids: Option<std::collections::BTreeMap<String, Vec<String>>>,
}

#[derive(Debug, Deserialize)]
struct WireStat {
    #[serde(rename = "ref")]
    reference: String,
    #[serde(default)]
    dp: bool,
    #[serde(default)]
    better: i8,
    matchers: Vec<WireMatcher>,
    #[serde(default)]
    trade: WireTrade,
}

#[derive(Debug, Deserialize)]
struct WireCraftable {
    #[serde(default)]
    category: Option<String>,
}

#[derive(Debug, Deserialize)]
struct WireMap {
    #[serde(default)]
    tier: Option<u32>,
}

#[derive(Debug, Deserialize)]
struct WireItem {
    name: String,
    #[serde(rename = "refName")]
    reference_name: String,
    namespace: String,
    #[serde(default, rename = "tradeDisc")]
    trade_disc: Option<String>,
    #[serde(default)]
    craftable: Option<WireCraftable>,
    #[serde(default)]
    map: Option<WireMap>,
}

/// The stat and item tables for one game.
#[derive(Debug, Default)]
pub struct GameTables {
    stats: Vec<Stat>,
    /// Matcher template to (stat index, matcher index).
    ///
    /// The first stat to claim a template keeps it. A duplicate template in
    /// the data is a data bug, and picking the later one would make the answer
    /// depend on file order.
    by_matcher: HashMap<String, (usize, usize)>,
    /// Lowercased name plus namespace to every item with that name.
    by_name: HashMap<(String, Namespace), Vec<BaseInfo>>,
}

impl GameTables {
    /// Load from a directory holding `stats.ndjson` and `items.ndjson`.
    pub fn load(dir: &Path) -> Result<Self, LoadError> {
        let stats = read_lines::<WireStat>(&dir.join("stats.ndjson"))?;
        let items = read_lines::<WireItem>(&dir.join("items.ndjson"))?;

        Ok(Self::from_wire(stats, items))
    }

    fn from_wire(wire_stats: Vec<WireStat>, wire_items: Vec<WireItem>) -> Self {
        let mut out = Self::default();

        for w in wire_stats {
            let stat = Stat {
                reference: w.reference,
                decimals: w.dp,
                better: StatBetter::from_i8(w.better),
                matchers: w
                    .matchers
                    .into_iter()
                    .map(|m| StatMatcher {
                        string: m.string,
                        advanced: m.advanced,
                        negate: m.negate,
                        value: m.value,
                    })
                    .collect(),
                trade: TradeInfo {
                    inverted: w.trade.inverted,
                    option: w.trade.option,
                    count: w.trade.count,
                    ids: w.trade.ids.unwrap_or_default(),
                },
            };

            let stat_idx = out.stats.len();

            for (matcher_idx, matcher) in stat.matchers.iter().enumerate() {
                out.by_matcher
                    .entry(matcher.string.clone())
                    .or_insert((stat_idx, matcher_idx));

                // The Advanced Item Description form is a second way to write
                // the same stat, so it has to reach the same entry.
                if let Some(advanced) = &matcher.advanced {
                    out.by_matcher
                        .entry(advanced.clone())
                        .or_insert((stat_idx, matcher_idx));
                }
            }

            out.stats.push(stat);
        }

        for w in wire_items {
            let Some(namespace) = Namespace::parse(&w.namespace) else {
                // An unknown namespace means our reader is older than the data.
                // Skipping the record beats refusing to start.
                continue;
            };

            let category = w
                .craftable
                .as_ref()
                .and_then(|c| c.category.as_deref())
                .and_then(ItemCategory::parse);

            let info = BaseInfo {
                name: w.name.clone(),
                reference_name: w.reference_name,
                namespace: w.namespace,
                trade_discriminator: w.trade_disc,
                craftable: w.craftable.is_some(),
                map_tier: w.map.and_then(|m| m.tier),
                category,
                // Roll ranges come from the game bundles and not from the
                // trade API, so they stay absent until those are vendored.
                armour_bounds: poe_trader_core::types::item::ArmourBounds::default(),
            };

            out.by_name
                .entry((w.name.to_lowercase(), namespace))
                .or_default()
                .push(info);
        }

        out
    }

    /// How many stats were loaded.
    pub fn stat_count(&self) -> usize {
        self.stats.len()
    }

    /// How many distinct item names were loaded.
    pub fn item_name_count(&self) -> usize {
        self.by_name.len()
    }
}

impl StatLookup for GameTables {
    fn stat_by_matcher(&self, template: &str) -> Option<StatHit<'_>> {
        let (stat_idx, matcher_idx) = *self.by_matcher.get(template)?;

        let stat = self.stats.get(stat_idx)?;
        let matcher = stat.matchers.get(matcher_idx)?;

        Some(StatHit { stat, matcher })
    }
}

impl ItemLookup for GameTables {
    fn items_by_name(&self, name: &str, namespace: Namespace, _game: GameVersion) -> Vec<BaseInfo> {
        self.by_name
            .get(&(name.to_lowercase(), namespace))
            .cloned()
            .unwrap_or_default()
    }
}

/// Read an ndjson file into a vector.
///
/// Blank lines are skipped, so a file ending in a newline loads cleanly.
fn read_lines<T: serde::de::DeserializeOwned>(path: &Path) -> Result<Vec<T>, LoadError> {
    let text = std::fs::read_to_string(path).map_err(|source| LoadError::Read {
        path: path.to_path_buf(),
        source,
    })?;

    let mut out = Vec::new();

    for (i, line) in text.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }

        let record = serde_json::from_str(line).map_err(|source| LoadError::Parse {
            path: path.to_path_buf(),
            line: i + 1,
            source,
        })?;

        out.push(record);
    }

    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(dir: &Path, name: &str, lines: &[&str]) {
        std::fs::write(dir.join(name), lines.join("\n")).unwrap();
    }

    fn tempdir() -> PathBuf {
        let base = std::env::temp_dir().join(format!(
            "poe-trader-test-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));

        std::fs::create_dir_all(&base).unwrap();

        base
    }

    const CHARM_SLOT: &str = r##"{"ref": "# Charm Slot", "better": 1, "matchers": [{"string": "# Charm Slots"}, {"string": "# Charm Slot", "value": 1}], "trade": {"ids": {"explicit": ["explicit.stat_2582079000"], "rune": ["rune.stat_554899692"]}}}"##;

    const LIFE: &str = r##"{"ref": "# to maximum Life", "dp": false, "better": 1, "matchers": [{"string": "# to maximum Life"}], "trade": {"ids": {"explicit": ["explicit.stat_3299347043"]}}}"##;

    const REDUCED: &str = r##"{"ref": "#% increased Attack Speed", "better": 1, "matchers": [{"string": "#% increased Attack Speed"}, {"string": "#% reduced Attack Speed", "negate": true}], "trade": {"ids": {"explicit": ["explicit.stat_210067635"]}}}"##;

    const CHAOS_ORB: &str = r##"{"name": "Chaos Orb", "refName": "Chaos Orb", "namespace": "ITEM", "icon": "x", "w": 1, "h": 1}"##;

    const WAYSTONE: &str = r##"{"name": "Waystone of Chaos", "refName": "Waystone of Chaos", "namespace": "ITEM", "icon": "x", "map": {"tier": 15}}"##;

    const KAOMS: &str = r##"{"name": "Kaom's Heart", "refName": "Kaom's Heart", "namespace": "UNIQUE", "icon": "x", "craftable": {"category": "Body Armour"}}"##;

    fn tables() -> GameTables {
        let dir = tempdir();
        write(&dir, "stats.ndjson", &[CHARM_SLOT, LIFE, REDUCED]);
        write(&dir, "items.ndjson", &[CHAOS_ORB, WAYSTONE, KAOMS]);

        GameTables::load(&dir).unwrap()
    }

    #[test]
    fn the_real_data_shape_loads() {
        // Both records are copied verbatim out of the reference's English
        // output, so this proves the wire types match the real file.
        let t = tables();

        assert_eq!(t.stat_count(), 3);
        assert_eq!(t.item_name_count(), 3);
    }

    #[test]
    fn a_stat_is_found_by_its_matcher() {
        let t = tables();

        let hit = t.stat_by_matcher("# to maximum Life").unwrap();

        assert_eq!(hit.stat.reference, "# to maximum Life");
        assert_eq!(hit.matcher.string, "# to maximum Life");
    }

    #[test]
    fn a_second_matcher_reaches_the_same_stat() {
        // "# Charm Slot" and "# Charm Slots" are one stat printed two ways.
        let t = tables();

        let plural = t.stat_by_matcher("# Charm Slots").unwrap();
        let singular = t.stat_by_matcher("# Charm Slot").unwrap();

        assert_eq!(plural.stat.reference, singular.stat.reference);
    }

    #[test]
    fn a_matcher_carries_its_baked_value() {
        // "# Charm Slot" only ever means one, and the data says so.
        let t = tables();

        let hit = t.stat_by_matcher("# Charm Slot").unwrap();

        assert_eq!(hit.matcher.value, Some(1.0));
    }

    #[test]
    fn a_negating_matcher_carries_its_flag() {
        let t = tables();

        let hit = t.stat_by_matcher("#% reduced Attack Speed").unwrap();

        assert!(hit.matcher.negate);
        assert_eq!(hit.stat.reference, "#% increased Attack Speed");
    }

    #[test]
    fn an_unknown_matcher_finds_nothing() {
        let t = tables();

        assert!(t.stat_by_matcher("# to maximum Sanity").is_none());
    }

    #[test]
    fn matching_is_exact_and_not_fuzzy() {
        // A near match is a different stat with a different trade id.
        let t = tables();

        assert!(t.stat_by_matcher("# to maximum life").is_none());
        assert!(t.stat_by_matcher(" # to maximum Life").is_none());
    }

    #[test]
    fn trade_ids_survive_the_load() {
        let t = tables();

        let hit = t.stat_by_matcher("# Charm Slots").unwrap();

        assert_eq!(
            hit.stat.trade.ids.get("explicit").map(Vec::as_slice),
            Some(["explicit.stat_2582079000".to_string()].as_slice())
        );
        assert!(hit.stat.trade.ids.contains_key("rune"));
    }

    #[test]
    fn a_stat_with_null_trade_ids_loads() {
        // The reference writes "ids": null for a stat that exists in the game
        // and has no trade filter. serde(default) alone rejects an explicit
        // null, so the real 1936 line file failed to load without this.
        let dir = tempdir().join("nullids");
        std::fs::create_dir_all(&dir).unwrap();

        write(
            &dir,
            "stats.ndjson",
            &[
                r##"{"ref": "Has Logbook Faction: Black Scythe Mercenaries", "better": 0, "matchers": [{"string": "Black Scythe Mercenaries"}], "trade": {"ids": null}}"##,
            ],
        );
        write(&dir, "items.ndjson", &[CHAOS_ORB]);

        let t = GameTables::load(&dir).unwrap();

        let hit = t.stat_by_matcher("Black Scythe Mercenaries").unwrap();
        assert!(hit.stat.trade.ids.is_empty());
    }

    #[test]
    fn an_item_is_found_by_name() {
        let t = tables();

        let got = t.items_by_name("Chaos Orb", Namespace::Item, GameVersion::Poe2);

        assert_eq!(got.len(), 1);
        assert_eq!(got[0].reference_name, "Chaos Orb");
    }

    #[test]
    fn item_lookup_ignores_case() {
        // Clipboard text casing is not guaranteed across game versions.
        let t = tables();

        assert_eq!(
            t.items_by_name("chaos orb", Namespace::Item, GameVersion::Poe2)
                .len(),
            1
        );
    }

    #[test]
    fn an_item_in_another_namespace_is_not_returned() {
        // The same name can exist in two tables. Returning the wrong one makes
        // the price check silently wrong.
        let t = tables();

        assert!(t
            .items_by_name("Chaos Orb", Namespace::Unique, GameVersion::Poe2)
            .is_empty());
    }

    #[test]
    fn a_map_base_carries_its_tier() {
        // A PoE2 waystone prints no tier, so the parser reads it from here.
        let t = tables();

        let got = t.items_by_name("Waystone of Chaos", Namespace::Item, GameVersion::Poe2);

        assert_eq!(got[0].map_tier, Some(15));
    }

    #[test]
    fn a_craftable_base_is_flagged_as_craftable() {
        let t = tables();

        let kaoms = t.items_by_name("Kaom's Heart", Namespace::Unique, GameVersion::Poe2);
        let orb = t.items_by_name("Chaos Orb", Namespace::Item, GameVersion::Poe2);

        assert!(kaoms[0].craftable);
        assert!(!orb[0].craftable);
    }

    #[test]
    fn a_base_carries_the_category_the_data_file_gives_it() {
        // The item text names an item class, not a trade category, and the two
        // do not line up. The data file holds the mapping.
        let t = tables();

        let got = t.items_by_name("Kaom's Heart", Namespace::Unique, GameVersion::Poe2);

        assert_eq!(got[0].category, Some(ItemCategory::BodyArmour));
    }

    #[test]
    fn a_base_with_an_unknown_category_reports_none() {
        // Newer data than our reader. Reporting a wrong category would send
        // the search to the wrong part of the trade site.
        let dir = tempdir().join("cat");
        std::fs::create_dir_all(&dir).unwrap();

        write(&dir, "stats.ndjson", &[LIFE]);
        write(
            &dir,
            "items.ndjson",
            &[
                r##"{"name": "Thing", "refName": "Thing", "namespace": "ITEM", "icon": "x", "craftable": {"category": "Brand New Slot"}}"##,
            ],
        );

        let t = GameTables::load(&dir).unwrap();
        let got = t.items_by_name("Thing", Namespace::Item, GameVersion::Poe2);

        assert!(got[0].craftable);
        assert_eq!(got[0].category, None);
    }

    #[test]
    fn an_unknown_item_finds_nothing() {
        let t = tables();

        assert!(t
            .items_by_name("Nonexistent", Namespace::Item, GameVersion::Poe2)
            .is_empty());
    }

    #[test]
    fn two_bases_with_one_name_are_both_returned() {
        // A Two-Stone Ring is fire and cold or fire and lightning, and only
        // the implicit tells them apart. Returning one would pick at random.
        let dir = tempdir().join("dup");
        std::fs::create_dir_all(&dir).unwrap();

        write(&dir, "stats.ndjson", &[LIFE]);
        write(
            &dir,
            "items.ndjson",
            &[
                r##"{"name": "Two-Stone Ring", "refName": "Two-Stone Ring", "namespace": "ITEM", "icon": "x", "tradeDisc": "fire_cold"}"##,
                r##"{"name": "Two-Stone Ring", "refName": "Two-Stone Ring", "namespace": "ITEM", "icon": "x", "tradeDisc": "fire_lightning"}"##,
            ],
        );

        let t = GameTables::load(&dir).unwrap();

        let got = t.items_by_name("Two-Stone Ring", Namespace::Item, GameVersion::Poe2);

        assert_eq!(got.len(), 2);
        assert_eq!(got[0].trade_discriminator.as_deref(), Some("fire_cold"));
    }

    #[test]
    fn an_unknown_namespace_is_skipped_rather_than_fatal() {
        // Newer data than our reader is not a reason to refuse to start.
        let dir = tempdir().join("ns");
        std::fs::create_dir_all(&dir).unwrap();

        write(&dir, "stats.ndjson", &[LIFE]);
        write(
            &dir,
            "items.ndjson",
            &[
                CHAOS_ORB,
                r##"{"name": "Future Thing", "refName": "Future Thing", "namespace": "TOTALLY_NEW", "icon": "x"}"##,
            ],
        );

        let t = GameTables::load(&dir).unwrap();

        assert_eq!(t.item_name_count(), 1);
    }

    #[test]
    fn a_trailing_newline_does_not_produce_an_empty_record() {
        let dir = tempdir().join("trailing");
        std::fs::create_dir_all(&dir).unwrap();

        std::fs::write(dir.join("stats.ndjson"), format!("{LIFE}\n")).unwrap();
        std::fs::write(dir.join("items.ndjson"), format!("{CHAOS_ORB}\n\n")).unwrap();

        let t = GameTables::load(&dir).unwrap();

        assert_eq!(t.stat_count(), 1);
        assert_eq!(t.item_name_count(), 1);
    }

    #[test]
    fn a_malformed_line_names_its_file_and_line_number() {
        // A 20000 line file with one bad record is otherwise impossible to fix.
        let dir = tempdir().join("bad");
        std::fs::create_dir_all(&dir).unwrap();

        write(&dir, "stats.ndjson", &[LIFE, "{not json", CHARM_SLOT]);
        write(&dir, "items.ndjson", &[CHAOS_ORB]);

        let err = GameTables::load(&dir).unwrap_err();

        let rendered = err.to_string();
        assert!(rendered.contains("stats.ndjson"), "{rendered}");
        assert!(rendered.contains("line 2"), "{rendered}");
    }

    #[test]
    fn a_missing_file_names_the_path() {
        let err = GameTables::load(Path::new("/nonexistent/poe-trader")).unwrap_err();

        assert!(err.to_string().contains("/nonexistent/poe-trader"));
    }

    #[test]
    fn the_first_stat_to_claim_a_template_keeps_it() {
        // A duplicate template is a data bug. Picking the later one would make
        // the answer depend on file order.
        let dir = tempdir().join("dupstat");
        std::fs::create_dir_all(&dir).unwrap();

        write(
            &dir,
            "stats.ndjson",
            &[
                r##"{"ref": "first", "better": 1, "matchers": [{"string": "# to maximum Life"}], "trade": {"ids": {}}}"##,
                r##"{"ref": "second", "better": 1, "matchers": [{"string": "# to maximum Life"}], "trade": {"ids": {}}}"##,
            ],
        );
        write(&dir, "items.ndjson", &[CHAOS_ORB]);

        let t = GameTables::load(&dir).unwrap();

        assert_eq!(
            t.stat_by_matcher("# to maximum Life")
                .unwrap()
                .stat
                .reference,
            "first"
        );
    }
}
