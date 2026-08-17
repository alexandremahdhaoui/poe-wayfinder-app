use poe_wayfinder_core::types::query::{
    Filters, Flag, NameField, Range, StatFilter, StatGroup, Status, TradeQuery,
};
use poe_wayfinder_core::types::GameVersion;
use serde_json::{json, Map, Value};

pub fn to_json(query: &TradeQuery, game: GameVersion) -> Value {
    let mut q = Map::new();

    q.insert("status".into(), json!({ "option": query.status.as_str() }));

    if let Some(name) = &query.name {
        q.insert("name".into(), name_to_json(name));
    }

    if let Some(type_name) = &query.type_name {
        q.insert("type".into(), name_to_json(type_name));
    }

    let stats: Vec<Value> = query.stats.iter().map(stat_group_to_json).collect();

    q.insert("stats".into(), Value::Array(stats));

    let filters = filters_to_json(&query.filters, game);

    if !filters.is_empty() {
        q.insert("filters".into(), Value::Object(filters));
    }

    json!({
        "query": Value::Object(q),
        "sort": { "price": "asc" },
    })
}

pub fn to_exchange_json(want: &str, have: &[String], status: Status) -> Value {
    let mut query = Map::new();

    query.insert(
        "status".into(),
        serde_json::json!({ "option": status.as_str() }),
    );
    query.insert("want".into(), serde_json::json!([want]));
    query.insert("have".into(), serde_json::json!(have));

    serde_json::json!({
        "query": Value::Object(query),
        "sort": { "have": "asc" },
        "engine": "new",
    })
}

fn name_to_json(name: &NameField) -> Value {
    match name {
        NameField::Plain(n) => Value::String(n.clone()),
        NameField::Discriminated(d) => json!({
            "option": d.option,
            "discriminator": d.discriminator,
        }),
    }
}

fn stat_group_to_json(group: &StatGroup) -> Value {
    let mut out = Map::new();

    out.insert("type".into(), Value::String(group.kind.as_str().into()));

    if let Some(value) = range_to_json(group.value) {
        out.insert("value".into(), value);
    }

    out.insert(
        "filters".into(),
        Value::Array(group.filters.iter().map(stat_filter_to_json).collect()),
    );

    if group.disabled {
        out.insert("disabled".into(), Value::Bool(true));
    }

    Value::Object(out)
}

fn stat_filter_to_json(filter: &StatFilter) -> Value {
    let mut out = Map::new();

    out.insert("id".into(), Value::String(filter.id.clone()));

    let mut value = Map::new();

    if let Some(min) = filter.range.min {
        value.insert("min".into(), number(min));
    }

    if let Some(max) = filter.range.max {
        value.insert("max".into(), number(max));
    }

    if let Some(option) = filter.option {
        value.insert("option".into(), number(option));
    }

    if !value.is_empty() {
        out.insert("value".into(), Value::Object(value));
    }

    if filter.disabled {
        out.insert("disabled".into(), Value::Bool(true));
    }

    Value::Object(out)
}

fn equipment_groups(game: GameVersion) -> (&'static str, &'static str) {
    match game {
        GameVersion::Poe2 => ("equipment_filters", "equipment_filters"),
        GameVersion::Poe1 => ("armour_filters", "weapon_filters"),
    }
}

fn filters_to_json(filters: &Filters, game: GameVersion) -> Map<String, Value> {
    let mut out = Map::new();

    let t = &filters.type_filters;
    let mut type_filters = Map::new();
    insert_option(&mut type_filters, "rarity", t.rarity.as_deref());
    insert_option(&mut type_filters, "category", t.category.as_deref());
    insert_range(&mut type_filters, "ilvl", t.ilvl);
    insert_range(&mut type_filters, "quality", t.quality);
    insert_group(&mut out, "type_filters", type_filters);

    let e = &filters.equipment_filters;
    let (defence_group, weapon_group) = equipment_groups(game);

    let mut defences = Map::new();
    insert_range(&mut defences, "ar", e.ar);
    insert_range(&mut defences, "ev", e.ev);
    insert_range(&mut defences, "es", e.es);
    insert_range(&mut defences, "block", e.block);

    let mut weapons = Map::new();
    insert_range(&mut weapons, "aps", e.aps);
    insert_range(&mut weapons, "crit", e.crit);
    insert_range(&mut weapons, "dps", e.dps);
    insert_range(&mut weapons, "edps", e.edps);
    insert_range(&mut weapons, "pdps", e.pdps);

    if game == GameVersion::Poe2 {
        insert_range(&mut weapons, "rune_sockets", e.rune_sockets);
        insert_range(&mut weapons, "spirit", e.spirit);
        insert_range(&mut weapons, "reload_time", e.reload_time);
    }

    if defence_group == weapon_group {
        defences.append(&mut weapons);
        insert_group(&mut out, defence_group, defences);
    } else {
        insert_group(&mut out, defence_group, defences);
        insert_group(&mut out, weapon_group, weapons);
    }

    let r = &filters.req_filters;
    let mut req = Map::new();
    insert_range(&mut req, "lvl", r.lvl);
    insert_range(&mut req, "str", r.str);
    insert_range(&mut req, "dex", r.dex);
    insert_range(&mut req, "int", r.int);
    insert_group(&mut out, "req_filters", req);

    let m = &filters.map_filters;
    let mut map = Map::new();
    insert_range(&mut map, "map_tier", m.map_tier);
    insert_range(&mut map, "map_revives", m.map_revives);
    insert_range(&mut map, "map_packsize", m.map_packsize);
    insert_range(&mut map, "map_magic_monsters", m.map_magic_monsters);
    insert_range(&mut map, "map_rare_monsters", m.map_rare_monsters);
    insert_range(&mut map, "map_bonus", m.map_bonus);
    insert_range(&mut map, "map_iir", m.map_iir);
    insert_option(&mut map, "ultimatum_hint", m.ultimatum_hint.as_deref());
    insert_group(&mut out, "map_filters", map);

    let x = &filters.misc_filters;
    let mut misc = Map::new();
    insert_flag(&mut misc, "alternate_art", x.alternate_art);
    insert_range(&mut misc, "area_level", x.area_level);
    insert_flag(&mut misc, "corrupted", x.corrupted);
    insert_range(&mut misc, "gem_level", x.gem_level);
    insert_range(&mut misc, "gem_sockets", x.gem_sockets);
    insert_flag(&mut misc, "identified", x.identified);
    insert_flag(&mut misc, "mirrored", x.mirrored);
    insert_flag(&mut misc, "sanctified", x.sanctified);
    insert_range(&mut misc, "sanctum_gold", x.sanctum_gold);
    insert_range(&mut misc, "unidentified_tier", x.unidentified_tier);
    insert_flag(&mut misc, "veiled", x.veiled);
    insert_flag(&mut misc, "fractured_item", x.fractured_item);
    insert_option_number(&mut misc, "has_empty_modifier", x.has_empty_modifier);
    insert_group(&mut out, "misc_filters", misc);

    let tr = &filters.trade_filters;
    let mut trade = Map::new();
    insert_flag(&mut trade, "collapse", tr.collapse);
    insert_option(&mut trade, "indexed", tr.indexed.as_deref());
    insert_range(&mut trade, "price", tr.price);
    insert_group(&mut out, "trade_filters", trade);

    out
}

fn insert_group(out: &mut Map<String, Value>, name: &str, filters: Map<String, Value>) {
    if filters.is_empty() {
        return;
    }

    out.insert(name.into(), json!({ "filters": Value::Object(filters) }));
}

fn insert_range(out: &mut Map<String, Value>, name: &str, range: Range) {
    if let Some(value) = range_to_json(range) {
        out.insert(name.into(), value);
    }
}

fn range_to_json(range: Range) -> Option<Value> {
    if range.is_empty() {
        return None;
    }

    let mut out = Map::new();

    if let Some(min) = range.min {
        out.insert("min".into(), number(min));
    }

    if let Some(max) = range.max {
        out.insert("max".into(), number(max));
    }

    Some(Value::Object(out))
}

fn insert_option(out: &mut Map<String, Value>, name: &str, value: Option<&str>) {
    if let Some(value) = value {
        out.insert(name.into(), json!({ "option": value }));
    }
}

fn insert_option_number(out: &mut Map<String, Value>, name: &str, value: Option<f64>) {
    if let Some(value) = value {
        out.insert(name.into(), json!({ "option": format!("{value:.0}") }));
    }
}

fn insert_flag(out: &mut Map<String, Value>, name: &str, flag: Flag) {
    if let Some(value) = flag {
        out.insert(name.into(), json!({ "option": value.to_string() }));
    }
}

fn number(value: f64) -> Value {
    if value.fract() == 0.0 && value.abs() < 9.0e15 {
        return json!(value as i64);
    }

    json!(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn poe2_puts_every_property_in_one_group() {
        let mut query = TradeQuery::default();
        query.filters.equipment_filters.ar = Range::at_least(450.0);
        query.filters.equipment_filters.pdps = Range::at_least(135.0);

        let got = to_json(&query, GameVersion::Poe2);
        let filters = &got["query"]["filters"];

        assert_eq!(filters["equipment_filters"]["filters"]["ar"]["min"], 450.0);
        assert_eq!(
            filters["equipment_filters"]["filters"]["pdps"]["min"],
            135.0
        );
        assert!(filters.get("armour_filters").is_none());
        assert!(filters.get("weapon_filters").is_none());
    }

    #[test]
    fn poe1_splits_defences_from_weapon_damage() {
        let mut query = TradeQuery::default();
        query.filters.equipment_filters.ar = Range::at_least(450.0);
        query.filters.equipment_filters.pdps = Range::at_least(135.0);

        let got = to_json(&query, GameVersion::Poe1);
        let filters = &got["query"]["filters"];

        assert_eq!(filters["armour_filters"]["filters"]["ar"]["min"], 450.0);
        assert_eq!(filters["weapon_filters"]["filters"]["pdps"]["min"], 135.0);
        assert!(
            filters.get("equipment_filters").is_none(),
            "poe1 must never see equipment_filters"
        );
    }

    #[test]
    fn poe2_keeps_the_defences_when_both_halves_are_merged() {
        let mut query = TradeQuery::default();
        query.filters.equipment_filters.ar = Range::at_least(450.0);
        query.filters.equipment_filters.es = Range::at_least(120.0);
        query.filters.equipment_filters.crit = Range::at_least(6.5);

        let got = to_json(&query, GameVersion::Poe2);
        let group = &got["query"]["filters"]["equipment_filters"]["filters"];

        assert_eq!(group["ar"]["min"], 450.0);
        assert_eq!(group["es"]["min"], 120.0);
        assert_eq!(group["crit"]["min"], 6.5);
    }

    #[test]
    fn the_poe2_only_keys_never_reach_poe1() {
        let mut query = TradeQuery::default();
        query.filters.equipment_filters.rune_sockets = Range::at_least(2.0);
        query.filters.equipment_filters.spirit = Range::at_least(100.0);
        query.filters.equipment_filters.reload_time = Range::at_least(0.5);

        let got = to_json(&query, GameVersion::Poe1);
        let filters = &got["query"]["filters"];

        for key in ["rune_sockets", "spirit", "reload_time"] {
            assert!(
                filters["weapon_filters"]["filters"].get(key).is_none(),
                "{key} must not be sent to poe1"
            );
            assert!(filters["armour_filters"]["filters"].get(key).is_none());
        }
    }

    #[test]
    fn a_group_that_constrains_nothing_is_not_sent_for_either_game() {
        for game in [GameVersion::Poe1, GameVersion::Poe2] {
            let got = to_json(&TradeQuery::default(), game);
            let filters = &got["query"]["filters"];

            for key in ["equipment_filters", "armour_filters", "weapon_filters"] {
                assert!(filters.get(key).is_none(), "{game:?} sent an empty {key}");
            }
        }
    }
    use poe_wayfinder_core::types::query::TradeQuery;

    fn body(query: &TradeQuery) -> Value {
        to_json(query, GameVersion::Poe2)
    }

    #[test]
    fn a_default_query_has_the_minimum_shape() {
        let got = body(&TradeQuery::default());

        assert_eq!(got["query"]["status"]["option"], "online");
        assert_eq!(got["sort"]["price"], "asc");
        assert!(got["query"]["stats"].is_array());
    }

    #[test]
    fn a_default_query_sends_no_filters_block() {
        let got = body(&TradeQuery::default());

        assert!(got["query"].get("filters").is_none());
    }

    #[test]
    fn the_status_reaches_the_body() {
        let query = TradeQuery {
            status: Status::Any,
            ..TradeQuery::default()
        };

        assert_eq!(body(&query)["query"]["status"]["option"], "any");
    }

    #[test]
    fn a_plain_name_is_a_string() {
        let query = TradeQuery {
            type_name: Some(NameField::new("Sapphire Ring", None)),
            ..TradeQuery::default()
        };

        assert_eq!(body(&query)["query"]["type"], "Sapphire Ring");
    }

    #[test]
    fn a_discriminated_name_is_an_object() {
        let query = TradeQuery {
            type_name: Some(NameField::new("Two-Stone Ring", Some("fire_cold"))),
            ..TradeQuery::default()
        };

        let got = body(&query);

        assert_eq!(got["query"]["type"]["option"], "Two-Stone Ring");
        assert_eq!(got["query"]["type"]["discriminator"], "fire_cold");
    }

    #[test]
    fn a_filter_group_is_nested_under_its_own_filters_key() {
        let mut query = TradeQuery::default();
        query.filters.type_filters.category = Some("weapon.bow".into());

        let got = body(&query);

        assert_eq!(
            got["query"]["filters"]["type_filters"]["filters"]["category"]["option"],
            "weapon.bow"
        );
    }

    #[test]
    fn a_group_that_constrains_nothing_is_dropped() {
        let mut query = TradeQuery::default();
        query.filters.type_filters.category = Some("weapon.bow".into());

        let got = body(&query);

        assert!(got["query"]["filters"].get("map_filters").is_none());
        assert!(got["query"]["filters"].get("equipment_filters").is_none());
    }

    #[test]
    fn a_range_sends_only_the_end_that_is_set() {
        let mut query = TradeQuery::default();
        query.filters.equipment_filters.ar = Range::at_least(450.0);

        let got = body(&query);
        let ar = &got["query"]["filters"]["equipment_filters"]["filters"]["ar"];

        assert_eq!(ar["min"], 450);
        assert!(ar.get("max").is_none());
    }

    #[test]
    fn a_two_ended_range_sends_both() {
        let mut query = TradeQuery::default();
        query.filters.map_filters.map_tier = Range::exactly(16.0);

        let got = body(&query);
        let tier = &got["query"]["filters"]["map_filters"]["filters"]["map_tier"];

        assert_eq!(tier["min"], 16);
        assert_eq!(tier["max"], 16);
    }

    #[test]
    fn an_empty_modifier_travels_under_misc_filters_never_under_stats() {
        let mut query = TradeQuery::default();
        query.filters.misc_filters.has_empty_modifier = Some(1.0);

        let got = body(&query);

        assert_eq!(
            got["query"]["filters"]["misc_filters"]["filters"]["has_empty_modifier"]["option"], "1",
            "an item.* id inside stats is refused with Unsupported stat domain"
        );
        assert!(
            got["query"]["stats"]
                .as_array()
                .is_none_or(|groups| groups.is_empty()),
            "it must not appear as a stat"
        );
    }

    #[test]
    fn a_flag_is_sent_as_a_string_option() {
        let mut query = TradeQuery::default();
        query.filters.misc_filters.corrupted = Some(true);

        let got = body(&query);

        assert_eq!(
            got["query"]["filters"]["misc_filters"]["filters"]["corrupted"]["option"],
            "true"
        );
    }

    #[test]
    fn a_false_flag_is_sent_and_is_not_the_same_as_absent() {
        let mut query = TradeQuery::default();
        query.filters.misc_filters.corrupted = Some(false);

        let got = body(&query);

        assert_eq!(
            got["query"]["filters"]["misc_filters"]["filters"]["corrupted"]["option"],
            "false"
        );
    }

    #[test]
    fn an_absent_flag_is_not_sent_at_all() {
        let mut query = TradeQuery::default();
        query.filters.misc_filters.corrupted = Some(true);

        let got = body(&query);
        let misc = &got["query"]["filters"]["misc_filters"]["filters"];

        assert!(misc.get("mirrored").is_none());
        assert!(misc.get("veiled").is_none());
    }

    #[test]
    fn a_stat_filter_carries_its_id_and_range() {
        let mut query = TradeQuery::default();
        query.stats.push(StatGroup::all(vec![StatFilter::range(
            "explicit.stat_life",
            Range::at_least(45.0),
        )]));

        let got = body(&query);
        let filter = &got["query"]["stats"][0]["filters"][0];

        assert_eq!(filter["id"], "explicit.stat_life");
        assert_eq!(filter["value"]["min"], 45);
        assert_eq!(got["query"]["stats"][0]["type"], "and");
    }

    #[test]
    fn a_disabled_stat_filter_says_so() {
        let mut filter = StatFilter::range("explicit.stat_life", Range::at_least(45.0));
        filter.disabled = true;

        let mut query = TradeQuery::default();
        query.stats.push(StatGroup::all(vec![filter]));

        let got = body(&query);

        assert_eq!(got["query"]["stats"][0]["filters"][0]["disabled"], true);
    }

    #[test]
    fn an_enabled_stat_filter_omits_the_disabled_key() {
        let mut query = TradeQuery::default();
        query.stats.push(StatGroup::all(vec![StatFilter::range(
            "explicit.stat_life",
            Range::at_least(45.0),
        )]));

        let got = body(&query);

        assert!(got["query"]["stats"][0]["filters"][0]
            .get("disabled")
            .is_none());
    }

    #[test]
    fn a_stat_filter_with_no_constraint_sends_no_value() {
        let mut query = TradeQuery::default();
        query.stats.push(StatGroup::all(vec![StatFilter::range(
            "explicit.stat_freeze",
            Range::default(),
        )]));

        let got = body(&query);

        assert!(got["query"]["stats"][0]["filters"][0]
            .get("value")
            .is_none());
    }

    #[test]
    fn a_stat_option_reaches_the_value() {
        let mut filter = StatFilter::range("explicit.stat_alloc", Range::default());
        filter.option = Some(42.0);

        let mut query = TradeQuery::default();
        query.stats.push(StatGroup::all(vec![filter]));

        let got = body(&query);

        assert_eq!(
            got["query"]["stats"][0]["filters"][0]["value"]["option"],
            42
        );
    }

    #[test]
    fn a_whole_number_renders_without_a_decimal_point() {
        assert_eq!(number(45.0), json!(45));
        assert_eq!(number(-45.0), json!(-45));
        assert_eq!(number(0.0), json!(0));
    }

    #[test]
    fn a_fractional_number_keeps_its_decimals() {
        assert_eq!(number(1.35), json!(1.35));
        assert_eq!(number(6.5), json!(6.5));
    }

    #[test]
    fn a_number_too_large_for_an_integer_stays_a_float() {
        let huge = 1.0e17;

        assert_eq!(number(huge), json!(huge));
    }

    #[test]
    fn a_realistic_query_serialises_whole() {
        let mut query = TradeQuery {
            type_name: Some(NameField::new("Spine Bow", None)),
            ..TradeQuery::default()
        };
        query.filters.type_filters.category = Some("weapon.bow".into());
        query.filters.type_filters.rarity = Some("nonunique".into());
        query.filters.misc_filters.corrupted = Some(false);
        query.filters.equipment_filters.pdps = Range::at_least(135.0);
        query.stats.push(StatGroup::all(vec![StatFilter::range(
            "explicit.stat_life",
            Range::at_least(45.0),
        )]));

        let got = body(&query);
        let text = serde_json::to_string(&got).unwrap();

        assert!(text.starts_with('{'));
        assert_eq!(got["query"]["type"], "Spine Bow");
        assert_eq!(
            got["query"]["filters"]["type_filters"]["filters"]["rarity"]["option"],
            "nonunique"
        );
        assert_eq!(
            got["query"]["filters"]["equipment_filters"]["filters"]["pdps"]["min"],
            135
        );
        assert_eq!(
            got["query"]["stats"][0]["filters"][0]["id"],
            "explicit.stat_life"
        );
    }

    #[test]
    fn an_exchange_request_names_what_is_wanted() {
        let got = to_exchange_json("divine", &[], Status::Online);

        assert_eq!(got["query"]["want"], serde_json::json!(["divine"]));
    }

    #[test]
    fn an_empty_have_list_asks_for_every_pair() {
        let got = to_exchange_json("divine", &[], Status::Online);

        assert_eq!(got["query"]["have"], serde_json::json!([]));
    }

    #[test]
    fn a_have_list_reaches_the_body() {
        let got = to_exchange_json("divine", &["chaos".to_string()], Status::Online);

        assert_eq!(got["query"]["have"], serde_json::json!(["chaos"]));
    }

    #[test]
    fn the_status_reaches_the_exchange_body() {
        assert_eq!(
            to_exchange_json("divine", &[], Status::Any)["query"]["status"]["option"],
            "any"
        );
    }

    #[test]
    fn an_exchange_request_asks_for_the_new_engine() {
        assert_eq!(
            to_exchange_json("divine", &[], Status::Online)["engine"],
            "new"
        );
    }

    #[test]
    fn an_exchange_request_sorts_cheapest_first() {
        assert_eq!(
            to_exchange_json("divine", &[], Status::Online)["sort"]["have"],
            "asc"
        );
    }

    #[test]
    fn an_exchange_request_carries_no_stat_filters() {
        let got = to_exchange_json("divine", &[], Status::Online);

        assert!(got["query"].get("stats").is_none());
        assert!(got["query"].get("filters").is_none());
    }
}
