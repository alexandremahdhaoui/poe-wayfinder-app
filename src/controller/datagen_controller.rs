use std::collections::BTreeMap;

use poe_trader_core::types::ItemCategory;
use serde::Deserialize;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum DatagenError {
    #[error("reading the {table} table")]
    Decode {
        table: &'static str,
        #[source]
        source: serde_json::Error,
    },

    #[error("the {table} table held no usable records")]
    Empty { table: &'static str },
}

#[derive(Debug, Deserialize)]
struct StatsResponse {
    result: Vec<StatGroupWire>,
}

#[derive(Debug, Deserialize)]
struct StatGroupWire {
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
    #[serde(default, rename = "type")]
    type_name: Option<String>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    disc: Option<String>,
    #[serde(default)]
    flags: Option<FlagsWire>,
}

#[derive(Debug, Deserialize)]
struct FlagsWire {
    #[serde(default)]
    unique: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct StatRecord {
    pub reference: String,
    pub trade_ids: BTreeMap<String, Vec<String>>,
    pub option: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct ItemRecord {
    pub name: String,
    pub namespace: String,
    pub trade_discriminator: Option<String>,
    pub category: Option<String>,
    pub craftable: bool,
    pub map_tier: Option<u32>,
    pub trade_tag: Option<String>,
}

pub fn build_trade_tags(body: &str) -> Result<BTreeMap<String, String>, DatagenError> {
    let parsed: StaticResponse =
        serde_json::from_str(body).map_err(|source| DatagenError::Decode {
            table: "static",
            source,
        })?;

    let mut out = BTreeMap::new();

    for group in parsed.result {
        for entry in group.entries {
            let (Some(text), Some(id)) = (entry.text, entry.id) else {
                continue;
            };

            if text.trim().is_empty() || id.trim().is_empty() {
                continue;
            }

            out.entry(text).or_insert(id);
        }
    }

    if out.is_empty() {
        return Err(DatagenError::Empty { table: "static" });
    }

    Ok(out)
}

#[derive(serde::Deserialize)]
struct StaticResponse {
    result: Vec<StaticGroup>,
}

#[derive(serde::Deserialize)]
struct StaticGroup {
    #[serde(default)]
    entries: Vec<StaticEntry>,
}

#[derive(serde::Deserialize)]
struct StaticEntry {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    text: Option<String>,
}

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

            merge_trade_ids_into(record, &group.id, &entry.id);

            if entry.option.as_ref().is_some_and(|o| !o.options.is_empty()) {
                record.option = true;
            }
        }
    }

    if by_text.is_empty() {
        return Err(DatagenError::Empty { table: "stats" });
    }

    Ok(by_text.into_values().collect())
}

fn merge_trade_ids_into(record: &mut StatRecord, namespace: &str, id: &str) {
    let ids = record.trade_ids.entry(namespace.to_string()).or_default();

    if ids.iter().any(|existing| existing == id) {
        return;
    }

    ids.push(id.to_string());
}

pub fn build_items(
    body: &str,
    trade_tags: &BTreeMap<String, String>,
) -> Result<Vec<ItemRecord>, DatagenError> {
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

            let name_for_tag = name.clone();

            out.push(ItemRecord {
                name,
                namespace: namespace_for(&group.id, is_unique).to_string(),
                trade_discriminator: entry.disc.clone(),
                category: category.map(|c| c.as_str().to_string()),
                craftable: is_craftable(&group.id),
                map_tier: map_tier_in(&name_for_tag),
                trade_tag: trade_tags.get(&name_for_tag).cloned(),
            });
        }
    }

    if out.is_empty() {
        return Err(DatagenError::Empty { table: "items" });
    }

    out.sort();
    out.dedup();

    Ok(out)
}

fn map_tier_in(name: &str) -> Option<u32> {
    let head = name.strip_suffix(')')?;
    let (_, digits) = head.rsplit_once(" (Tier ")?;

    if digits.is_empty() || !digits.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }

    digits.parse().ok()
}

fn is_craftable(group_id: &str) -> bool {
    matches!(
        group_id,
        "accessory"
            | "armour"
            | "weapon"
            | "flask"
            | "jewel"
            | "map"
            | "tincture"
            | "idol"
            | "heistequipment"
    )
}

fn namespace_for(group_id: &str, is_unique: bool) -> &'static str {
    if is_unique {
        return "UNIQUE";
    }

    match group_id {
        "gem" => "GEM",
        "card" => "DIVINATION_CARD",
        "monster" => "CAPTURED_BEAST",
        _ => "ITEM",
    }
}

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

fn category_from_name(group_id: &str, name: &str) -> Option<ItemCategory> {
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

pub fn item_to_ndjson(record: &ItemRecord) -> String {
    let mut value = serde_json::json!({
        "name": record.name,
        "refName": record.name,
        "namespace": record.namespace,
    });

    if let Some(disc) = &record.trade_discriminator {
        value["tradeDisc"] = serde_json::Value::String(disc.clone());
    }

    if let Some(tag) = &record.trade_tag {
        value["tradeTag"] = serde_json::Value::String(tag.clone());
    }

    if record.craftable {
        value["craftable"] = match &record.category {
            Some(category) => serde_json::json!({ "category": category }),
            None => serde_json::json!({}),
        };
    }

    if let Some(category) = &record.category {
        value["category"] = serde_json::Value::String(category.clone());
    }

    if let Some(tier) = record.map_tier {
        value["map"] = serde_json::json!({ "tier": tier });
    }

    value.to_string()
}

#[derive(Debug, Deserialize)]
struct SourceAugmentEffect {
    categories: Vec<String>,
    string: String,
    #[serde(default)]
    values: Vec<f64>,
    #[serde(rename = "tradeId", default)]
    trade_id: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct SourceCraftable {
    #[serde(default)]
    category: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SourceAugmentItem {
    name: String,
    #[serde(rename = "refName")]
    reference_name: String,
    #[serde(default)]
    craftable: Option<SourceCraftable>,
    #[serde(default)]
    augment: Option<Vec<SourceAugmentEffect>>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AugmentEffectRecord {
    pub reference: String,
    pub trade_id: String,
    pub value: f64,
    pub categories: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AugmentRecord {
    pub name: String,
    pub reference_name: String,
    pub effects: Vec<AugmentEffectRecord>,
}

pub fn build_augments(ndjson: &str) -> Vec<AugmentRecord> {
    let mut out = Vec::new();

    for line in ndjson.lines() {
        if line.trim().is_empty() {
            continue;
        }

        let Ok(item) = serde_json::from_str::<SourceAugmentItem>(line) else {
            continue;
        };

        if item.craftable.and_then(|c| c.category).as_deref() != Some("SoulCore") {
            continue;
        }

        let Some(effects) = item.augment else {
            continue;
        };

        let effects: Vec<AugmentEffectRecord> = effects
            .into_iter()
            .filter(|e| !e.trade_id.is_empty())
            .filter_map(|e| {
                let categories: Vec<String> = e
                    .categories
                    .iter()
                    .filter_map(|c| ItemCategory::parse(c).map(|c| c.as_str().to_string()))
                    .collect();

                if categories.is_empty() {
                    return None;
                }

                let trade_id = e.trade_id.first()?.clone();

                Some(AugmentEffectRecord {
                    reference: e.string,
                    trade_id,
                    value: e.values.first().copied().unwrap_or(0.0),
                    categories,
                })
            })
            .collect();

        if effects.is_empty() {
            continue;
        }

        out.push(AugmentRecord {
            name: item.name,
            reference_name: item.reference_name,
            effects,
        });
    }

    out.sort_by(|a, b| a.reference_name.cmp(&b.reference_name));
    out.dedup_by(|a, b| a.reference_name == b.reference_name);

    out
}

pub fn augment_to_ndjson(record: &AugmentRecord) -> String {
    let effects: Vec<serde_json::Value> = record
        .effects
        .iter()
        .map(|e| {
            serde_json::json!({
                "reference": e.reference,
                "tradeId": e.trade_id,
                "value": e.value,
                "categories": e.categories,
            })
        })
        .collect();

    serde_json::json!({
        "name": record.name,
        "refName": record.reference_name,
        "effects": effects,
    })
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

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
        {"id":"gem","label":"Gems","entries":[
            {"type":"Awakened Fire Penetration Support"}
        ]},
        {"id":"card","label":"Cards","entries":[
            {"type":"The Doctor"}
        ]},
        {"id":"monster","label":"Itemised Monsters","entries":[
            {"type":"Cave Beast"}
        ]},
        {"id":"something.new","label":"Unmapped","entries":[
            {"type":"Mystery Thing"}
        ]}
    ]}"##;

    fn stats() -> Vec<StatRecord> {
        build_stats(STATS).unwrap()
    }

    fn items() -> Vec<ItemRecord> {
        build_items(ITEMS, &BTreeMap::new()).unwrap()
    }

    #[test]
    fn the_live_stats_shape_builds() {
        assert_eq!(stats().len(), 3);
    }

    #[test]
    fn one_stat_merges_every_namespace_it_appears_in() {
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
        let err = build_stats(r#"{"result":[]}"#).unwrap_err();

        assert!(matches!(err, DatagenError::Empty { table: "stats" }));
    }

    #[test]
    fn the_live_items_shape_builds() {
        assert_eq!(items().len(), 9);
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
    fn a_waystone_carries_the_tier_from_its_name() {
        assert_eq!(map_tier_in("Waystone (Tier 16)"), Some(16));
        assert_eq!(map_tier_in("Waystone (Tier 1)"), Some(1));
    }

    #[test]
    fn a_base_with_no_tier_in_its_name_has_none() {
        assert_eq!(map_tier_in("Sapphire Ring"), None);
        assert_eq!(map_tier_in("Waystone"), None);
    }

    #[test]
    fn a_tier_that_is_not_a_number_is_not_a_tier() {
        assert_eq!(map_tier_in("Thing (Tier of Doom)"), None);
        assert_eq!(map_tier_in("Thing (Tier )"), None);
    }

    #[test]
    fn a_tier_must_be_at_the_end_of_the_name() {
        assert_eq!(map_tier_in("Waystone (Tier 3) of Doom"), None);
    }

    #[test]
    fn a_weapon_base_is_craftable() {
        let bow = items().into_iter().find(|i| i.name == "Spine Bow").unwrap();

        assert!(bow.craftable);
    }

    #[test]
    fn an_accessory_is_craftable() {
        let ring = items()
            .into_iter()
            .find(|i| i.name == "Sapphire Ring")
            .unwrap();

        assert!(ring.craftable);
    }

    #[test]
    fn a_gem_is_not_craftable() {
        let gem = items()
            .into_iter()
            .find(|i| i.name == "Awakened Fire Penetration Support")
            .unwrap();

        assert!(!gem.craftable);
    }

    #[test]
    fn a_divination_card_is_not_craftable() {
        let card = items()
            .into_iter()
            .find(|i| i.name == "The Doctor")
            .unwrap();

        assert!(!card.craftable);
    }

    #[test]
    fn a_craftable_base_with_no_known_category_still_says_so() {
        let bow = items().into_iter().find(|i| i.name == "Spine Bow").unwrap();

        assert!(bow.category.is_none());
        assert!(bow.craftable);

        let json = item_to_ndjson(&bow);

        assert!(json.contains("craftable"), "{json}");
    }

    #[test]
    fn a_gem_is_filed_in_the_gem_table() {
        let gem = items()
            .into_iter()
            .find(|i| i.name == "Awakened Fire Penetration Support")
            .unwrap();

        assert_eq!(gem.namespace, "GEM");
    }

    #[test]
    fn a_divination_card_is_filed_in_the_card_table() {
        let card = items()
            .into_iter()
            .find(|i| i.name == "The Doctor")
            .unwrap();

        assert_eq!(card.namespace, "DIVINATION_CARD");
    }

    #[test]
    fn an_itemised_monster_is_filed_in_the_beast_table() {
        let beast = items()
            .into_iter()
            .find(|i| i.name == "Cave Beast")
            .unwrap();

        assert_eq!(beast.namespace, "CAPTURED_BEAST");
    }

    #[test]
    fn every_namespace_written_is_one_the_parser_reads() {
        for item in items() {
            assert!(
                poe_trader_core::adapter::data_adapter::Namespace::parse(&item.namespace).is_some(),
                "{} was filed under {}",
                item.name,
                item.namespace
            );
        }
    }

    #[test]
    fn a_weapon_base_carries_no_category_from_this_source() {
        let bow = items().into_iter().find(|i| i.name == "Spine Bow").unwrap();

        assert_eq!(bow.category, None);
    }

    #[test]
    fn a_unique_is_filed_by_its_own_name_in_the_unique_namespace() {
        let unique = items()
            .into_iter()
            .find(|i| i.namespace == "UNIQUE")
            .unwrap();

        assert_eq!(unique.name, "Kaom's Sign");
    }

    #[test]
    fn both_discriminated_variants_survive() {
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
        let mystery = items()
            .into_iter()
            .find(|i| i.name == "Mystery Thing")
            .unwrap();

        assert_eq!(mystery.category, None);
    }

    #[test]
    fn duplicate_entries_are_collapsed() {
        let body = r##"{"result":[
            {"id":"accessory","entries":[{"type":"Sapphire Ring"}]},
            {"id":"accessory","entries":[{"type":"Sapphire Ring"}]}
        ]}"##;

        assert_eq!(build_items(body, &BTreeMap::new()).unwrap().len(), 1);
    }

    #[test]
    fn an_entry_with_no_name_at_all_is_dropped() {
        let body = r##"{"result":[{"id":"accessory","entries":[
            {},{"type":""},{"type":"Sapphire Ring"}
        ]}]}"##;

        assert_eq!(build_items(body, &BTreeMap::new()).unwrap().len(), 1);
    }

    #[test]
    fn an_empty_items_response_is_an_error() {
        let err = build_items(r#"{"result":[]}"#, &BTreeMap::new()).unwrap_err();

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
        assert_eq!(category_for_group("accessory"), None);
        assert_eq!(category_for_group("armour"), None);
        assert_eq!(category_for_group("weapon"), None);
        assert_eq!(category_for_group(""), None);
    }

    #[test]
    fn an_accessory_resolves_from_its_name() {
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
        assert_eq!(category_from_name("armour", "Iron Greathelm"), None);
        assert_eq!(category_from_name("weapon", "Spine Bow"), None);
    }

    #[test]
    fn an_unrecognised_accessory_ending_yields_nothing() {
        assert_eq!(category_from_name("accessory", "Heavy Chain"), None);
        assert_eq!(category_from_name("accessory", ""), None);
    }
    const ADEPT_RUNE: &str = r##"{"name":"Adept Rune","refName":"Adept Rune","craftable":{"category":"SoulCore"},"augment":[{"categories":["Body Armour","Bow"],"string":"# to Dexterity","values":[9],"tradeId":["rune.stat_3261801346"]}]}"##;

    const NO_TRADE_ID: &str = r##"{"name":"Mystery Rune","refName":"Mystery Rune","craftable":{"category":"SoulCore"},"augment":[{"categories":["Body Armour"],"string":"# to Nothing","values":[1]}]}"##;

    const NOT_A_RUNE: &str =
        r##"{"name":"Chaos Orb","refName":"Chaos Orb","craftable":{"category":"Currency"}}"##;

    #[test]
    fn a_rune_with_a_trade_id_becomes_an_augment() {
        let got = build_augments(ADEPT_RUNE);

        assert_eq!(got.len(), 1);
        assert_eq!(got[0].name, "Adept Rune");
        assert_eq!(got[0].effects[0].reference, "# to Dexterity");
        assert_eq!(got[0].effects[0].trade_id, "rune.stat_3261801346");
        assert_eq!(got[0].effects[0].value, 9.0);
    }

    #[test]
    fn an_augment_keeps_every_category_it_fits() {
        let got = build_augments(ADEPT_RUNE);

        assert!(got[0].effects[0]
            .categories
            .contains(&"Body Armour".to_string()));
        assert!(got[0].effects[0].categories.contains(&"Bow".to_string()));
    }

    #[test]
    fn an_effect_with_no_trade_id_is_dropped_because_it_cannot_be_searched() {
        assert!(build_augments(NO_TRADE_ID).is_empty());
    }

    #[test]
    fn an_item_that_is_not_a_socketable_is_not_an_augment() {
        assert!(build_augments(NOT_A_RUNE).is_empty());
    }

    #[test]
    fn a_line_that_is_not_json_is_skipped_rather_than_stopping_the_build() {
        let body = format!("{ADEPT_RUNE}\n{{not json\n{NOT_A_RUNE}");

        assert_eq!(build_augments(&body).len(), 1);
    }

    #[test]
    fn an_empty_line_is_skipped() {
        assert_eq!(build_augments(&format!("\n{ADEPT_RUNE}\n\n")).len(), 1);
    }

    #[test]
    fn the_same_rune_twice_is_written_once() {
        assert_eq!(
            build_augments(&format!("{ADEPT_RUNE}\n{ADEPT_RUNE}")).len(),
            1
        );
    }

    #[test]
    fn augments_are_written_in_a_stable_order() {
        let two = format!(
            "{ADEPT_RUNE}\n{}",
            ADEPT_RUNE.replace("Adept Rune", "Abyssal Rune")
        );
        let got = build_augments(&two);

        assert_eq!(got[0].reference_name, "Abyssal Rune");
        assert_eq!(got[1].reference_name, "Adept Rune");
    }

    #[test]
    fn an_augment_round_trips_through_ndjson() {
        let record = &build_augments(ADEPT_RUNE)[0];
        let line = augment_to_ndjson(record);

        assert!(line.contains("\"refName\":\"Adept Rune\""), "{line}");
        assert!(line.contains("\"value\":9.0"), "{line}");
        assert!(line.contains("# to Dexterity"), "{line}");
        assert!(line.contains("rune.stat_3261801346"), "{line}");
    }

    #[test]
    fn a_category_the_parser_does_not_know_is_dropped() {
        let odd = ADEPT_RUNE.replace("\"Bow\"", "\"Nonsense Category\"");
        let got = build_augments(&odd);

        assert_eq!(
            got[0].effects[0].categories,
            vec!["Body Armour".to_string()]
        );
    }

    #[test]
    fn an_effect_with_no_known_category_at_all_is_dropped() {
        let odd = ADEPT_RUNE.replace("[\"Body Armour\",\"Bow\"]", "[\"Nonsense\"]");

        assert!(build_augments(&odd).is_empty());
    }
}
