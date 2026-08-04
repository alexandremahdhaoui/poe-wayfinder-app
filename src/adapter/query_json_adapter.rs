//! Serialising a trade query to the JSON the API accepts.
//!
//! The domain owns the query as a plain type. This turns it into wire JSON.
//! Keeping the two apart means the API's odd shape never leaks into the
//! filters, and a future API change is one file.
//!
//! # Every empty filter is omitted
//!
//! The API rejects an unknown key and silently ignores a filter block whose
//! inner shape is wrong. It also treats an empty range object as a real
//! constraint on some endpoints. Sending only what the user actually asked for
//! is the only shape that behaves the same on every endpoint.

use poe_trader_core::types::query::{
    Filters, Flag, NameField, Range, StatFilter, StatGroup, TradeQuery,
};
use serde_json::{json, Map, Value};

/// Turn a query into the request body.
pub fn to_json(query: &TradeQuery) -> Value {
    let mut q = Map::new();

    q.insert("status".into(), json!({ "option": query.status.as_str() }));

    if let Some(name) = &query.name {
        q.insert("name".into(), name_to_json(name));
    }

    if let Some(type_name) = &query.type_name {
        q.insert("type".into(), name_to_json(type_name));
    }

    let stats: Vec<Value> = query.stats.iter().map(stat_group_to_json).collect();

    // The stats key is always sent, even empty. The API treats a missing stats
    // array as malformed rather than as no stat filters.
    q.insert("stats".into(), Value::Array(stats));

    let filters = filters_to_json(&query.filters);

    if !filters.is_empty() {
        q.insert("filters".into(), Value::Object(filters));
    }

    json!({
        "query": Value::Object(q),
        "sort": { "price": "asc" },
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

/// Build the filter groups, dropping every group that constrains nothing.
fn filters_to_json(filters: &Filters) -> Map<String, Value> {
    let mut out = Map::new();

    let t = &filters.type_filters;
    let mut type_filters = Map::new();
    insert_option(&mut type_filters, "rarity", t.rarity.as_deref());
    insert_option(&mut type_filters, "category", t.category.as_deref());
    insert_range(&mut type_filters, "ilvl", t.ilvl);
    insert_range(&mut type_filters, "quality", t.quality);
    insert_group(&mut out, "type_filters", type_filters);

    let e = &filters.equipment_filters;
    let mut equipment = Map::new();
    insert_range(&mut equipment, "aps", e.aps);
    insert_range(&mut equipment, "ar", e.ar);
    insert_range(&mut equipment, "block", e.block);
    insert_range(&mut equipment, "crit", e.crit);
    insert_range(&mut equipment, "dps", e.dps);
    insert_range(&mut equipment, "edps", e.edps);
    insert_range(&mut equipment, "es", e.es);
    insert_range(&mut equipment, "ev", e.ev);
    insert_range(&mut equipment, "pdps", e.pdps);
    insert_range(&mut equipment, "rune_sockets", e.rune_sockets);
    insert_range(&mut equipment, "spirit", e.spirit);
    insert_range(&mut equipment, "reload_time", e.reload_time);
    insert_group(&mut out, "equipment_filters", equipment);

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
    insert_group(&mut out, "misc_filters", misc);

    let tr = &filters.trade_filters;
    let mut trade = Map::new();
    insert_flag(&mut trade, "collapse", tr.collapse);
    insert_option(&mut trade, "indexed", tr.indexed.as_deref());
    insert_range(&mut trade, "price", tr.price);
    insert_group(&mut out, "trade_filters", trade);

    out
}

/// Wrap a group's filters under the nested `filters` key the API expects.
///
/// A group that constrains nothing is dropped. Sending an empty group is
/// accepted on some endpoints and rejected on others.
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

fn insert_flag(out: &mut Map<String, Value>, name: &str, flag: Flag) {
    if let Some(value) = flag {
        // The API takes the string "true" or "false" here and not a bool. A
        // real bool is accepted and then ignored, so the filter silently does
        // nothing.
        out.insert(name.into(), json!({ "option": value.to_string() }));
    }
}

/// Render a number without a trailing `.0` when it is whole.
///
/// The API accepts both. A whole number reads better in a shared trade link,
/// which users paste to each other constantly.
fn number(value: f64) -> Value {
    if value.fract() == 0.0 && value.abs() < 9.0e15 {
        return json!(value as i64);
    }

    json!(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use poe_trader_core::types::query::{Status, TradeQuery};

    fn body(query: &TradeQuery) -> Value {
        to_json(query)
    }

    #[test]
    fn a_default_query_has_the_minimum_shape() {
        let got = body(&TradeQuery::default());

        assert_eq!(got["query"]["status"]["option"], "online");
        assert_eq!(got["sort"]["price"], "asc");
        // The API treats a missing stats array as malformed.
        assert!(got["query"]["stats"].is_array());
    }

    #[test]
    fn a_default_query_sends_no_filters_block() {
        // An empty filters block is accepted on some endpoints and rejected on
        // others.
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
        // Sending the bare name searches both variants and prices the wrong
        // one half the time.
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
        // The nesting is the API's and it rejects the flatter shape.
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
    fn a_flag_is_sent_as_a_string_option() {
        // The API accepts a real bool here and then ignores it, so the filter
        // silently does nothing.
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
        // Sending false where absent was meant excludes every item that has
        // the property.
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
        // It still travels so the trade site shows it greyed out when the user
        // opens the link.
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
        // A presence check. Sending an empty value object is rejected.
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
        // Users paste trade links to each other constantly and 45 reads better
        // than 45.0.
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
        // Casting it would silently change the value.
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

        // The whole body has to be one object the API accepts.
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
}
