use std::collections::HashMap;
use std::path::{Path, PathBuf};

use poe_trader_core::adapter::data_adapter::{ItemLookup, Namespace, StatLookup};
use poe_trader_core::controller::filter::augments::{Augment, AugmentEffect};
use poe_trader_core::types::item::BaseInfo;
use poe_trader_core::types::stat::{Stat, StatBetter, StatHit, StatMatcher, TradeInfo};
use poe_trader_core::types::ItemCategory;
use poe_trader_core::types::{GamePair, GameVersion};
use serde::Deserialize;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum LoadError {
    #[error("reading {path}")]
    Read {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("parsing {path} line {line}")]
    Parse {
        path: PathBuf,
        line: usize,
        #[source]
        source: serde_json::Error,
    },
}

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
    #[serde(default, rename = "tradeTag")]
    trade_tag: Option<String>,
    #[serde(default)]
    craftable: Option<WireCraftable>,
    #[serde(default)]
    category: Option<String>,
    #[serde(default)]
    map: Option<WireMap>,
}

#[derive(Debug, serde::Deserialize)]
struct WireAugmentEffect {
    reference: String,
    #[serde(rename = "tradeId")]
    trade_id: String,
    value: f64,
    categories: Vec<String>,
}

#[derive(Debug, serde::Deserialize)]
struct WireAugment {
    #[serde(rename = "refName")]
    reference_name: String,
    name: String,
    effects: Vec<WireAugmentEffect>,
}

#[derive(Debug, Default)]
pub struct GameTables {
    stats: Vec<Stat>,
    by_matcher: HashMap<String, (usize, usize)>,
    by_name: HashMap<(String, Namespace), Vec<BaseInfo>>,
    augments: Vec<Augment>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Origin {
    Directory,
    Cache,
    Embedded,
}

impl Origin {
    pub fn as_str(self) -> &'static str {
        match self {
            Origin::Directory => "directory",
            Origin::Cache => "cache",
            Origin::Embedded => "embedded",
        }
    }
}

pub fn resolve_both(
    dir: &str,
    config_dir: &Path,
    pinned: Option<GameVersion>,
) -> Result<(GamePair<GameTables>, GamePair<Origin>), LoadError> {
    let named_for = |game: GameVersion| match pinned {
        Some(only) if only != game => "",
        _ => dir,
    };

    let (poe1, first) =
        GameTables::resolve(named_for(GameVersion::Poe1), config_dir, GameVersion::Poe1)?;
    let (poe2, second) =
        GameTables::resolve(named_for(GameVersion::Poe2), config_dir, GameVersion::Poe2)?;

    Ok((GamePair::new(poe1, poe2), GamePair::new(first, second)))
}

pub fn cache_dir(config_dir: &Path, game: GameVersion) -> PathBuf {
    config_dir.join(format!("data-{}", game.as_str()))
}

impl GameTables {
    pub fn load(dir: &Path) -> Result<Self, LoadError> {
        let stats = read_lines::<WireStat>(&dir.join("stats.ndjson"))?;
        let items = read_lines::<WireItem>(&dir.join("items.ndjson"))?;

        let mut tables = Self::from_wire(stats, items);
        tables.augments = load_augments(&dir.join("augments.ndjson"))?;

        Ok(tables)
    }

    pub fn embedded(game: GameVersion) -> Result<Self, LoadError> {
        let name = game.as_str();
        let root = Path::new("<built in>").join(name);

        let stats = parse_bytes::<WireStat>(poe_trader_data::stats(name), &root.join("stats"))?;
        let items = parse_bytes::<WireItem>(poe_trader_data::items(name), &root.join("items"))?;

        let mut tables = Self::from_wire(stats, items);
        tables.augments = Self::embedded_augments(game);

        Ok(tables)
    }

    fn embedded_augments(game: GameVersion) -> Vec<Augment> {
        let path = Path::new("<built in>").join(game.as_str()).join("augments");

        match parse_bytes::<WireAugment>(poe_trader_data::augments(game.as_str()), &path) {
            Ok(wire) => augments_from_wire(wire),
            Err(_) => Vec::new(),
        }
    }

    pub fn resolve(
        dir: &str,
        config_dir: &Path,
        game: GameVersion,
    ) -> Result<(Self, Origin), LoadError> {
        if !dir.trim().is_empty() {
            return Self::load(Path::new(dir)).map(|t| (t, Origin::Directory));
        }

        let cache = cache_dir(config_dir, game);

        if cache.join("stats.ndjson").exists() {
            if let Ok(mut tables) = Self::load(&cache) {
                if tables.augments.is_empty() {
                    tables.augments = Self::embedded_augments(game);
                }

                return Ok((tables, Origin::Cache));
            }
        }

        Self::embedded(game).map(|t| (t, Origin::Embedded))
    }

    pub fn augment_count(&self) -> usize {
        self.augments.len()
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
                continue;
            };

            let category = w
                .category
                .as_deref()
                .or_else(|| w.craftable.as_ref().and_then(|c| c.category.as_deref()))
                .and_then(ItemCategory::parse);

            let info = BaseInfo {
                name: w.name.clone(),
                reference_name: w.reference_name,
                namespace: w.namespace,
                trade_discriminator: w.trade_disc,
                trade_tag: w.trade_tag,
                craftable: w.craftable.is_some(),
                map_tier: w.map.and_then(|m| m.tier),
                category,
                armour_bounds: poe_trader_core::types::item::ArmourBounds::default(),
                unique_base: None,
            };

            out.by_name
                .entry((w.name.to_lowercase(), namespace))
                .or_default()
                .push(info);
        }

        out
    }

    pub fn stat_count(&self) -> usize {
        self.stats.len()
    }

    pub fn item_name_count(&self) -> usize {
        self.by_name.len()
    }

    pub fn items(&self) -> impl Iterator<Item = (Namespace, &BaseInfo)> {
        self.by_name
            .iter()
            .flat_map(|((_, ns), bases)| bases.iter().map(move |b| (*ns, b)))
    }

    pub fn matchers(&self) -> impl Iterator<Item = (&str, &str)> {
        self.stats.iter().flat_map(|stat| {
            stat.matchers
                .iter()
                .map(move |m| (stat.reference.as_str(), m.string.as_str()))
        })
    }
}

impl StatLookup for GameTables {
    fn stat_count(&self) -> usize {
        self.stats.len()
    }

    fn stat_by_matcher(&self, template: &str) -> Option<StatHit<'_>> {
        let (stat_idx, matcher_idx) = *self.by_matcher.get(template)?;

        let stat = self.stats.get(stat_idx)?;
        let matcher = stat.matchers.get(matcher_idx)?;

        Some(StatHit { stat, matcher })
    }
}

impl ItemLookup for GameTables {
    fn item_name_count(&self) -> usize {
        self.by_name.len()
    }

    fn items_by_name(&self, name: &str, namespace: Namespace, _game: GameVersion) -> Vec<BaseInfo> {
        self.by_name
            .get(&(name.to_lowercase(), namespace))
            .cloned()
            .unwrap_or_default()
    }

    fn augments(&self) -> &[Augment] {
        &self.augments
    }
}

fn load_augments(path: &Path) -> Result<Vec<Augment>, LoadError> {
    if !path.exists() {
        return Ok(Vec::new());
    }

    Ok(augments_from_wire(read_lines::<WireAugment>(path)?))
}

fn augments_from_wire(wire: Vec<WireAugment>) -> Vec<Augment> {
    wire.into_iter()
        .map(|w| Augment {
            reference_name: w.reference_name,
            name: w.name,
            effects: w
                .effects
                .into_iter()
                .map(|e| AugmentEffect {
                    reference: e.reference,
                    trade_id: e.trade_id,
                    value: e.value,
                    categories: e
                        .categories
                        .iter()
                        .filter_map(|c| ItemCategory::parse(c))
                        .collect(),
                })
                .collect(),
        })
        .filter(|a: &Augment| !a.effects.is_empty())
        .collect()
}

fn read_lines<T: serde::de::DeserializeOwned>(path: &Path) -> Result<Vec<T>, LoadError> {
    let text = std::fs::read_to_string(path).map_err(|source| LoadError::Read {
        path: path.to_path_buf(),
        source,
    })?;

    parse_lines(&text, path)
}

fn parse_bytes<T: serde::de::DeserializeOwned>(
    bytes: &[u8],
    path: &Path,
) -> Result<Vec<T>, LoadError> {
    let text = std::str::from_utf8(bytes).map_err(|_| LoadError::Read {
        path: path.to_path_buf(),
        source: std::io::Error::new(std::io::ErrorKind::InvalidData, "not utf8"),
    })?;

    parse_lines(text, path)
}

fn parse_lines<T: serde::de::DeserializeOwned>(
    text: &str,
    path: &Path,
) -> Result<Vec<T>, LoadError> {
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

    fn scratch(name: &str) -> PathBuf {
        let dir = tempdir().join(name);

        let _ = std::fs::remove_dir_all(&dir);

        std::fs::create_dir_all(&dir).unwrap();

        dir
    }

    #[test]
    fn both_games_load_from_inside_the_binary() {
        for game in [GameVersion::Poe1, GameVersion::Poe2] {
            let tables = GameTables::embedded(game).expect("the built in data parses");

            assert!(
                tables.stat_count() > 1000,
                "{game:?} {}",
                tables.stat_count()
            );
            assert!(tables.item_name_count() > 1000, "{game:?}");
        }
    }

    #[test]
    fn the_two_games_do_not_share_a_built_in_table() {
        let one = GameTables::embedded(GameVersion::Poe1).unwrap();
        let two = GameTables::embedded(GameVersion::Poe2).unwrap();

        assert_ne!(one.stat_count(), two.stat_count());
    }

    #[test]
    fn only_poe2_carries_built_in_augments() {
        assert_eq!(
            GameTables::embedded(GameVersion::Poe1)
                .unwrap()
                .augment_count(),
            0
        );
        assert!(
            GameTables::embedded(GameVersion::Poe2)
                .unwrap()
                .augment_count()
                > 0
        );
    }

    #[test]
    fn an_empty_data_dir_falls_back_to_the_built_in_copy() {
        let config = scratch("resolve-empty");

        let (tables, origin) = GameTables::resolve("", &config, GameVersion::Poe2).unwrap();

        assert_eq!(origin, Origin::Embedded);
        assert!(tables.stat_count() > 1000);
    }

    #[test]
    fn a_named_data_dir_wins_over_everything() {
        let dir = scratch("resolve-named");

        write(&dir, "stats.ndjson", &[LIFE]);
        write(&dir, "items.ndjson", &[CHAOS_ORB]);

        let (tables, origin) = GameTables::resolve(
            dir.to_str().unwrap(),
            &scratch("resolve-named-cfg"),
            GameVersion::Poe2,
        )
        .unwrap();

        assert_eq!(origin, Origin::Directory);
        assert_eq!(tables.stat_count(), 1);
    }

    #[test]
    fn a_named_data_dir_that_is_missing_is_fatal_rather_than_silently_replaced() {
        let config = scratch("resolve-missing");

        let got = GameTables::resolve("/nonexistent/poe-trader", &config, GameVersion::Poe2);

        assert!(
            got.is_err(),
            "a directory asked for by name must not be swapped out"
        );
    }

    #[test]
    fn the_cache_wins_over_the_built_in_copy() {
        let config = scratch("resolve-cache");
        let cache = cache_dir(&config, GameVersion::Poe2);

        std::fs::create_dir_all(&cache).unwrap();
        write(&cache, "stats.ndjson", &[LIFE, REDUCED]);
        write(&cache, "items.ndjson", &[CHAOS_ORB]);

        let (tables, origin) = GameTables::resolve("", &config, GameVersion::Poe2).unwrap();

        assert_eq!(origin, Origin::Cache);
        assert_eq!(tables.stat_count(), 2);
    }

    #[test]
    fn a_cache_without_augments_keeps_the_built_in_ones() {
        let config = scratch("resolve-cache-augments");
        let cache = cache_dir(&config, GameVersion::Poe2);

        std::fs::create_dir_all(&cache).unwrap();
        write(&cache, "stats.ndjson", &[LIFE]);
        write(&cache, "items.ndjson", &[CHAOS_ORB]);

        let (tables, _) = GameTables::resolve("", &config, GameVersion::Poe2).unwrap();

        assert!(
            tables.augment_count() > 0,
            "the refresh never writes augments, so the item editor would go empty"
        );
    }

    #[test]
    fn a_corrupt_cache_falls_back_instead_of_refusing_to_start() {
        let config = scratch("resolve-corrupt");
        let cache = cache_dir(&config, GameVersion::Poe2);

        std::fs::create_dir_all(&cache).unwrap();
        write(&cache, "stats.ndjson", &["{not json"]);
        write(&cache, "items.ndjson", &[CHAOS_ORB]);

        let (tables, origin) = GameTables::resolve("", &config, GameVersion::Poe2).unwrap();

        assert_eq!(origin, Origin::Embedded);
        assert!(tables.stat_count() > 1000);
    }

    #[test]
    fn both_games_are_held_at_once_from_the_built_in_copy() {
        let (pair, _) = resolve_both("", &scratch("pair-both"), None).expect("both games parse");

        assert!(pair.get(GameVersion::Poe1).stat_count() > 1000);
        assert!(pair.get(GameVersion::Poe2).stat_count() > 1000);
        assert_ne!(
            pair.get(GameVersion::Poe1).stat_count(),
            pair.get(GameVersion::Poe2).stat_count()
        );
    }

    #[test]
    fn a_named_data_dir_pinned_to_one_game_leaves_the_other_built_in() {
        let dir = scratch("pair-pinned");

        write(&dir, "stats.ndjson", &[LIFE]);
        write(&dir, "items.ndjson", &[CHAOS_ORB]);

        let (pair, _) = resolve_both(
            dir.to_str().unwrap(),
            &scratch("pair-pinned-cfg"),
            Some(GameVersion::Poe1),
        )
        .unwrap();

        assert_eq!(pair.get(GameVersion::Poe1).stat_count(), 1);
        assert!(
            pair.get(GameVersion::Poe2).stat_count() > 1000,
            "a directory built for one game must not be served to the other"
        );
    }

    #[test]
    fn with_no_data_dir_both_halves_of_the_pair_come_from_inside_the_binary() {
        let (pair, origin) = resolve_both("", &scratch("pair-empty"), None).unwrap();

        assert_eq!(*origin.get(GameVersion::Poe1), Origin::Embedded);
        assert_eq!(*origin.get(GameVersion::Poe2), Origin::Embedded);
        assert!(pair.get(GameVersion::Poe2).augment_count() > 0);
    }

    #[test]
    fn one_game_falling_back_does_not_relabel_the_other() {
        let config = scratch("pair-mixed");
        let good = cache_dir(&config, GameVersion::Poe1);
        let bad = cache_dir(&config, GameVersion::Poe2);

        for cache in [&good, &bad] {
            std::fs::create_dir_all(cache).unwrap();
            write(cache, "items.ndjson", &[CHAOS_ORB]);
        }

        write(&good, "stats.ndjson", &[LIFE]);
        write(&bad, "stats.ndjson", &["{not json"]);

        let (_, origin) = resolve_both("", &config, None).unwrap();

        assert_eq!(*origin.get(GameVersion::Poe1), Origin::Cache);
        assert_eq!(*origin.get(GameVersion::Poe2), Origin::Embedded);
    }

    #[test]
    fn every_origin_names_itself_for_the_log() {
        for origin in [Origin::Directory, Origin::Cache, Origin::Embedded] {
            assert!(!origin.as_str().is_empty(), "{origin:?}");
        }
    }

    #[test]
    fn the_cache_directory_is_per_game() {
        let config = Path::new("/cfg");

        assert_ne!(
            cache_dir(config, GameVersion::Poe1),
            cache_dir(config, GameVersion::Poe2)
        );
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
        let t = tables();

        let plural = t.stat_by_matcher("# Charm Slots").unwrap();
        let singular = t.stat_by_matcher("# Charm Slot").unwrap();

        assert_eq!(plural.stat.reference, singular.stat.reference);
    }

    #[test]
    fn a_matcher_carries_its_baked_value() {
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
        let t = tables();

        assert_eq!(
            t.items_by_name("chaos orb", Namespace::Item, GameVersion::Poe2)
                .len(),
            1
        );
    }

    #[test]
    fn an_item_in_another_namespace_is_not_returned() {
        let t = tables();

        assert!(t
            .items_by_name("Chaos Orb", Namespace::Unique, GameVersion::Poe2)
            .is_empty());
    }

    #[test]
    fn a_map_base_carries_its_tier() {
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
        let t = tables();

        let got = t.items_by_name("Kaom's Heart", Namespace::Unique, GameVersion::Poe2);

        assert_eq!(got[0].category, Some(ItemCategory::BodyArmour));
    }

    #[test]
    fn a_base_with_an_unknown_category_reports_none() {
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
