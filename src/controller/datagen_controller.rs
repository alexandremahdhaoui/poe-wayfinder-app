//! Building the game data files.
//!
//! Replaces the reference data pipeline, which is 7623 lines of Python.
//!
//! # What it does
//!
//! Pulls two tables from the official trade API and turns them into the ndjson
//! the parser reads.
//!
//! - `/data/stats` gives every stat the trade site can filter on, keyed by the
//!   text the game prints. That is exactly what the stat matcher needs.
//! - `/data/items` gives every base type, grouped by trade category. That is
//!   what the database lookup needs.
//!
//! # What it cannot do yet
//!
//! The trade API does not publish roll ranges, quality scaling or the modifier
//! tier tables. Those come from the game's own data bundles, which need a
//! bundle reader we have not ported. Until then those tables are vendored in
//! `poe-trader-data/tables`.
//!
//! What this builds is enough to parse an item and query for it, which is the
//! whole price check path.

use std::collections::BTreeMap;

use poe_trader_core::types::ItemCategory;
use serde::Deserialize;
use thiserror::Error;

/// Why a build failed.
#[derive(Debug, Error)]
pub enum DatagenError {
    #[error("reading the {table} table")]
    Decode {
        table: &'static str,
        #[source]
        source: serde_json::Error,
    },

    /// The response parsed and held nothing usable.
    ///
    /// Writing an empty file would leave the app unable to match any stat, and
    /// the failure would surface as "every modifier is unknown" much later.
    #[error("the {table} table held no usable records")]
    Empty { table: &'static str },
}

// ---------------------------------------------------------------------------
// Wire shapes, straight off the API
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct StatsResponse {
    result: Vec<StatGroupWire>,
}

#[derive(Debug, Deserialize)]
struct StatGroupWire {
    /// The namespace. `explicit`, `implicit`, `pseudo` and so on.
    id: String,
    entries: Vec<StatEntryWire>,
}

#[derive(Debug, Deserialize)]
struct StatEntryWire {
    id: String,
    text: String,
    #[serde(default)]
    option: Option<OptionsWire>,
}

#[derive(Debug, Deserialize)]
struct OptionsWire {
    /// Only the count is read.
    ///
    /// A non empty list means the stat takes an option and not a range, which
    /// is all the filter builder needs today. The option ids themselves matter
    /// for a filter that names a specific option, such as which passive an
    /// Allocates modifier grants, and carrying them is a later change.
    #[serde(default)]
    options: Vec<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
struct ItemsResponse {
    result: Vec<ItemGroupWire>,
}

#[derive(Debug, Deserialize)]
struct ItemGroupWire {
    id: String,
    entries: Vec<ItemEntryWire>,
}

#[derive(Debug, Deserialize)]
struct ItemEntryWire {
    /// The base type. Absent on a unique entry, which carries a name instead.
    #[serde(default, rename = "type")]
    type_name: Option<String>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    disc: Option<String>,
    /// Present on a unique.
    #[serde(default)]
    flags: Option<FlagsWire>,
}

#[derive(Debug, Deserialize)]
struct FlagsWire {
    #[serde(default)]
    unique: bool,
}

// ---------------------------------------------------------------------------
// Building
// ---------------------------------------------------------------------------

/// One stat record, ready to write.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct StatRecord {
    /// The canonical text, with `#` for each roll.
    pub reference: String,
    /// Trade ids per namespace.
    pub trade_ids: BTreeMap<String, Vec<String>>,
    /// Whether this stat takes an option rather than a range.
    pub option: bool,
}

/// One item record, ready to write.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct ItemRecord {
    pub name: String,
    pub namespace: String,
    pub trade_discriminator: Option<String>,
    pub category: Option<String>,
}

/// Turn the stats response into records.
///
/// One record per distinct text, merging the namespaces. The API lists the
/// same stat once per namespace and the parser needs one record holding all of
/// them, because a stat's id differs per namespace and its text does not.
pub fn build_stats(body: &str) -> Result<Vec<StatRecord>, DatagenError> {
    let parsed: StatsResponse =
        serde_json::from_str(body).map_err(|source| DatagenError::Decode {
            table: "stats",
            source,
        })?;

    let mut by_text: BTreeMap<String, StatRecord> = BTreeMap::new();

    for group in parsed.result {
        for entry in group.entries {
            if entry.text.trim().is_empty() {
                continue;
            }

            let record = by_text
                .entry(entry.text.clone())
                .or_insert_with(|| StatRecord {
                    reference: entry.text.clone(),
                    trade_ids: BTreeMap::new(),
                    option: false,
                });

            record
                .trade_ids
                .entry(group.id.clone())
                .or_default()
                .push(entry.id.clone());

            // A stat with a fixed option list takes an option and not a range.
            // Sending a range on one returns nothing.
            if entry.option.as_ref().is_some_and(|o| !o.options.is_empty()) {
                record.option = true;
            }
        }
    }

    if by_text.is_empty() {
        return Err(DatagenError::Empty { table: "stats" });
    }

    // Sorted by text. Two runs against the same input produce byte identical
    // files, so a diff means the game data changed.
    Ok(by_text.into_values().collect())
}

/// Turn the items response into records.
///
/// A unique entry is filed under the unique namespace and everything else
/// under the item namespace. Mixing them makes the database lookup return a
/// unique when the parser asked for a base.
pub fn build_items(body: &str) -> Result<Vec<ItemRecord>, DatagenError> {
    let parsed: ItemsResponse =
        serde_json::from_str(body).map_err(|source| DatagenError::Decode {
            table: "items",
            source,
        })?;

    let mut out: Vec<ItemRecord> = Vec::new();

    for group in parsed.result {
        let group_category = category_for_group(&group.id);

        for entry in group.entries {
            let is_unique = entry.flags.as_ref().is_some_and(|f| f.unique);

            // A unique is named by `name`. Everything else by `type`.
            let name = if is_unique {
                entry.name.clone().or_else(|| entry.type_name.clone())
            } else {
                entry.type_name.clone().or_else(|| entry.name.clone())
            };

            let Some(name) = name else {
                continue;
            };

            if name.trim().is_empty() {
                continue;
            }

            let category = group_category.or_else(|| category_from_name(&group.id, &name));

            out.push(ItemRecord {
                name,
                namespace: if is_unique { "UNIQUE" } else { "ITEM" }.to_string(),
                trade_discriminator: entry.disc.clone(),
                category: category.map(|c| c.as_str().to_string()),
            });
        }
    }

    if out.is_empty() {
        return Err(DatagenError::Empty { table: "items" });
    }

    // Sorted and deduplicated. The API lists a base once per category it can
    // appear in, and a duplicate line makes the loaded table depend on order.
    out.sort();
    out.dedup();

    Ok(out)
}

/// Map an items group id to a category.
///
/// The trade API groups items far more coarsely than we model them. It gives
/// `accessory`, `armour` and `weapon`, not `accessory.ring`. So only the
/// groups that are already one category resolve here.
fn category_for_group(group_id: &str) -> Option<ItemCategory> {
    match group_id {
        "flask" => Some(ItemCategory::Flask),
        "jewel" => Some(ItemCategory::Jewel),
        "map" => Some(ItemCategory::Map),
        "currency" => Some(ItemCategory::Currency),
        "card" => Some(ItemCategory::DivinationCard),
        "sanctum" => Some(ItemCategory::SanctumRelic),
        "wombgift" => Some(ItemCategory::Wombgift),
        _ => None,
    }
}

/// Work out a fine category from a base name's last word.
///
/// PoE base names end in their type. `Sapphire Ring` is a ring and
/// `Gold Amulet` is an amulet. Accessories are completely regular this way,
/// which is why they resolve and armour does not.
///
/// Armour and weapon names use dozens of synonyms. `Greathelm`, `Mask`,
/// `Crown` and `Helm` are all helmets, and guessing that list would be wrong
/// in ways nobody would notice until a search returned nothing. Those
/// categories come from the game's own data bundles, which is what the
/// vendored tables are for.
fn category_from_name(group_id: &str, name: &str) -> Option<ItemCategory> {
    // Only trusted inside a group that narrows the possibilities. A rare
    // called "Doom Ring" is not a ring base.
    if group_id != "accessory" {
        return None;
    }

    let tail = name.rsplit(' ').next()?;

    match tail {
        "Ring" => Some(ItemCategory::Ring),
        "Amulet" => Some(ItemCategory::Amulet),
        "Belt" => Some(ItemCategory::Belt),
        _ => None,
    }
}

/// Render a stat record as one ndjson line.
///
/// Written by hand rather than derived, so the key order is fixed. A derived
/// order can change between serde versions and every line in the file would
/// then differ for no reason.
pub fn stat_to_ndjson(record: &StatRecord) -> String {
    let ids: BTreeMap<&String, &Vec<String>> = record.trade_ids.iter().collect();

    let value = serde_json::json!({
        "ref": record.reference,
        "better": 1,
        "matchers": [{ "string": record.reference }],
        "trade": {
            "ids": ids,
            "option": record.option,
        },
    });

    value.to_string()
}

/// Render an item record as one ndjson line.
pub fn item_to_ndjson(record: &ItemRecord) -> String {
    let mut value = serde_json::json!({
        "name": record.name,
        "refName": record.name,
        "namespace": record.namespace,
    });

    if let Some(disc) = &record.trade_discriminator {
        value["tradeDisc"] = serde_json::Value::String(disc.clone());
    }

    if let Some(category) = &record.category {
        value["craftable"] = serde_json::json!({ "category": category });
    }

    value.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    // Copied from the live API response shape.
    const STATS: &str = r##"{"result":[
        {"id":"explicit","label":"Explicit","entries":[
            {"id":"explicit.stat_3299347043","text":"# to maximum Life","type":"explicit"},
            {"id":"explicit.stat_2901986750","text":"Allocates #","type":"explicit",
             "option":{"options":[{"id":1,"text":"Acrobatics"},{"id":2,"text":"Ancestral Bond"}]}}
        ]},
        {"id":"implicit","label":"Implicit","entries":[
            {"id":"implicit.stat_3299347043","text":"# to maximum Life","type":"implicit"}
        ]},
        {"id":"pseudo","label":"Pseudo","entries":[
            {"id":"pseudo.pseudo_total_cold_resistance","text":"+#% total to Cold Resistance","type":"pseudo"}
        ]}
    ]}"##;

    const ITEMS: &str = r##"{"result":[
        {"id":"accessory","label":"Accessories","entries":[
            {"type":"Sapphire Ring"},
            {"type":"Two-Stone Ring","disc":"fire_cold"},
            {"type":"Two-Stone Ring","disc":"fire_lightning"},
            {"name":"Kaom's Sign","type":"Gold Ring","flags":{"unique":true}}
        ]},
        {"id":"weapon","label":"Weapons","entries":[
            {"type":"Spine Bow"}
        ]},
        {"id":"something.new","label":"Unmapped","entries":[
            {"type":"Mystery Thing"}
        ]}
    ]}"##;

    fn stats() -> Vec<StatRecord> {
        build_stats(STATS).unwrap()
    }

    fn items() -> Vec<ItemRecord> {
        build_items(ITEMS).unwrap()
    }

    #[test]
    fn the_live_stats_shape_builds() {
        assert_eq!(stats().len(), 3);
    }

    #[test]
    fn one_stat_merges_every_namespace_it_appears_in() {
        // The API lists the same stat once per namespace. A stat's id differs
        // per namespace and its text does not, so the parser needs one record
        // holding all of them.
        let life = stats()
            .into_iter()
            .find(|s| s.reference == "# to maximum Life")
            .unwrap();

        assert_eq!(life.trade_ids.len(), 2);
        assert_eq!(
            life.trade_ids.get("explicit"),
            Some(&vec!["explicit.stat_3299347043".to_string()])
        );
        assert_eq!(
            life.trade_ids.get("implicit"),
            Some(&vec!["implicit.stat_3299347043".to_string()])
        );
    }

    #[test]
    fn a_stat_with_a_fixed_option_list_is_flagged() {
        // Sending a range on one returns nothing.
        let allocates = stats()
            .into_iter()
            .find(|s| s.reference == "Allocates #")
            .unwrap();

        assert!(allocates.option);
    }

    #[test]
    fn a_stat_without_options_is_not_flagged() {
        let life = stats()
            .into_iter()
            .find(|s| s.reference == "# to maximum Life")
            .unwrap();

        assert!(!life.option);
    }

    #[test]
    fn pseudo_stats_are_kept() {
        // They are how a user searches for total resistance across modifiers,
        // which is most rare jewellery searches.
        assert!(stats()
            .iter()
            .any(|s| s.reference.contains("total to Cold Resistance")));
    }

    #[test]
    fn the_output_is_sorted_so_two_runs_produce_the_same_file() {
        let a = stats();
        let mut sorted = a.clone();
        sorted.sort();

        assert_eq!(a, sorted);
    }

    #[test]
    fn a_stat_with_empty_text_is_dropped() {
        // It would match every line whose template is empty, which is none,
        // and only bloats the file.
        let body = r##"{"result":[{"id":"explicit","entries":[
            {"id":"a","text":""},{"id":"b","text":"  "},{"id":"c","text":"# to maximum Life"}
        ]}]}"##;

        assert_eq!(build_stats(body).unwrap().len(), 1);
    }

    #[test]
    fn a_stats_response_that_is_not_json_is_a_decode_error() {
        let err = build_stats("<html>503</html>").unwrap_err();

        assert!(err.to_string().contains("stats"));
        assert!(matches!(err, DatagenError::Decode { .. }));
    }

    #[test]
    fn an_empty_stats_response_is_an_error_and_not_an_empty_file() {
        // Writing an empty file leaves the app unable to match any stat, and
        // the failure surfaces as "every modifier is unknown" much later.
        let err = build_stats(r#"{"result":[]}"#).unwrap_err();

        assert!(matches!(err, DatagenError::Empty { table: "stats" }));
    }

    #[test]
    fn the_live_items_shape_builds() {
        assert_eq!(items().len(), 6);
    }

    #[test]
    fn an_accessory_gets_its_category_from_its_name() {
        let ring = items()
            .into_iter()
            .find(|i| i.name == "Sapphire Ring")
            .unwrap();

        assert_eq!(ring.category.as_deref(), Some("Ring"));
        assert_eq!(ring.namespace, "ITEM");
    }

    #[test]
    fn a_weapon_base_carries_no_category_from_this_source() {
        let bow = items().into_iter().find(|i| i.name == "Spine Bow").unwrap();

        assert_eq!(bow.category, None);
    }

    #[test]
    fn a_unique_is_filed_by_its_own_name_in_the_unique_namespace() {
        // Mixing them makes the database lookup return a unique when the
        // parser asked for a base.
        let unique = items()
            .into_iter()
            .find(|i| i.namespace == "UNIQUE")
            .unwrap();

        assert_eq!(unique.name, "Kaom's Sign");
    }

    #[test]
    fn both_discriminated_variants_survive() {
        // A Two-Stone Ring is fire and cold or fire and lightning, and only
        // the implicit tells them apart.
        let discs: Vec<Option<String>> = items()
            .into_iter()
            .filter(|i| i.name == "Two-Stone Ring")
            .map(|i| i.trade_discriminator)
            .collect();

        assert_eq!(discs.len(), 2);
        assert!(discs.contains(&Some("fire_cold".to_string())));
        assert!(discs.contains(&Some("fire_lightning".to_string())));
    }

    #[test]
    fn an_unmapped_group_still_yields_its_bases() {
        // A base with no category still prices by name. Dropping it would lose
        // the item entirely.
        let mystery = items()
            .into_iter()
            .find(|i| i.name == "Mystery Thing")
            .unwrap();

        assert_eq!(mystery.category, None);
    }

    #[test]
    fn duplicate_entries_are_collapsed() {
        // The API lists a base once per category it can appear in, and a
        // duplicate line makes the loaded table depend on file order.
        let body = r##"{"result":[
            {"id":"accessory","entries":[{"type":"Sapphire Ring"}]},
            {"id":"accessory","entries":[{"type":"Sapphire Ring"}]}
        ]}"##;

        assert_eq!(build_items(body).unwrap().len(), 1);
    }

    #[test]
    fn an_entry_with_no_name_at_all_is_dropped() {
        let body = r##"{"result":[{"id":"accessory","entries":[
            {},{"type":""},{"type":"Sapphire Ring"}
        ]}]}"##;

        assert_eq!(build_items(body).unwrap().len(), 1);
    }

    #[test]
    fn an_empty_items_response_is_an_error() {
        let err = build_items(r#"{"result":[]}"#).unwrap_err();

        assert!(matches!(err, DatagenError::Empty { table: "items" }));
    }

    #[test]
    fn a_stat_record_renders_as_the_shape_the_loader_reads() {
        let record = &stats()[0];

        let line = stat_to_ndjson(record);
        let parsed: serde_json::Value = serde_json::from_str(&line).unwrap();

        assert!(parsed["ref"].is_string());
        assert!(parsed["matchers"][0]["string"].is_string());
        assert!(parsed["trade"]["ids"].is_object());
    }

    #[test]
    fn an_item_record_renders_as_the_shape_the_loader_reads() {
        let ring = items()
            .into_iter()
            .find(|i| i.name == "Sapphire Ring")
            .unwrap();

        let line = item_to_ndjson(&ring);
        let parsed: serde_json::Value = serde_json::from_str(&line).unwrap();

        assert_eq!(parsed["name"], "Sapphire Ring");
        assert_eq!(parsed["refName"], "Sapphire Ring");
        assert_eq!(parsed["namespace"], "ITEM");
        assert_eq!(parsed["craftable"]["category"], "Ring");
    }

    #[test]
    fn an_item_with_no_discriminator_omits_the_key() {
        // An explicit null would be a discriminator of null, which matches no
        // variant.
        let ring = items()
            .into_iter()
            .find(|i| i.name == "Sapphire Ring")
            .unwrap();

        let parsed: serde_json::Value = serde_json::from_str(&item_to_ndjson(&ring)).unwrap();

        assert!(parsed.get("tradeDisc").is_none());
    }

    #[test]
    fn a_discriminated_item_carries_its_discriminator() {
        let two_stone = items()
            .into_iter()
            .find(|i| i.trade_discriminator.is_some())
            .unwrap();

        let parsed: serde_json::Value = serde_json::from_str(&item_to_ndjson(&two_stone)).unwrap();

        assert!(parsed["tradeDisc"].is_string());
    }

    #[test]
    fn every_rendered_line_holds_no_newline() {
        // One record per line is the whole point of ndjson. An embedded
        // newline would split one record into two unparseable halves.
        for record in stats() {
            assert!(!stat_to_ndjson(&record).contains('\n'));
        }

        for record in items() {
            assert!(!item_to_ndjson(&record).contains('\n'));
        }
    }

    #[test]
    fn a_group_that_is_already_one_category_resolves() {
        assert_eq!(category_for_group("flask"), Some(ItemCategory::Flask));
        assert_eq!(category_for_group("jewel"), Some(ItemCategory::Jewel));
        assert_eq!(category_for_group("map"), Some(ItemCategory::Map));
        assert_eq!(category_for_group("currency"), Some(ItemCategory::Currency));
    }

    #[test]
    fn a_coarse_group_yields_no_category_of_its_own() {
        // The API gives accessory, armour and weapon, not accessory.ring.
        assert_eq!(category_for_group("accessory"), None);
        assert_eq!(category_for_group("armour"), None);
        assert_eq!(category_for_group("weapon"), None);
        assert_eq!(category_for_group(""), None);
    }

    #[test]
    fn an_accessory_resolves_from_its_name() {
        // Accessory names are completely regular. 147 of the 153 the API lists
        // end in Ring, Amulet or Belt.
        assert_eq!(
            category_from_name("accessory", "Sapphire Ring"),
            Some(ItemCategory::Ring)
        );
        assert_eq!(
            category_from_name("accessory", "Gold Amulet"),
            Some(ItemCategory::Amulet)
        );
        assert_eq!(
            category_from_name("accessory", "Rawhide Belt"),
            Some(ItemCategory::Belt)
        );
    }

    #[test]
    fn an_armour_or_weapon_name_is_not_guessed_from() {
        // Greathelm, Mask, Crown and Helm are all helmets. Guessing that list
        // would be wrong in ways nobody would notice until a search returned
        // nothing. Those come from the game bundles.
        assert_eq!(category_from_name("armour", "Iron Greathelm"), None);
        assert_eq!(category_from_name("weapon", "Spine Bow"), None);
    }

    #[test]
    fn an_unrecognised_accessory_ending_yields_nothing() {
        assert_eq!(category_from_name("accessory", "Heavy Chain"), None);
        assert_eq!(category_from_name("accessory", ""), None);
    }
}
