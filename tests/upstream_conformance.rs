//! Exiled Exchange 2's own expected numbers for its own 26 test items.
//!
//! `fixtures/upstream_items.json` is harvested from
//! `renderer/specs/Parser/items.ts`, which declares each item's raw text and
//! then, on the lines below it, what the reference expects to be read from it.
//!
//! # Why this lives in the app crate
//!
//! The reference's own suite calls `init("en")` first: every one of its parser
//! tests runs with the full stat table loaded. A modifier whose stat does not
//! resolve is not recorded as a modifier at all, in the reference or here, so
//! running these without data would compare against a different question.
//!
//! Loading a data file is I/O, and `poe-trader-core` is not allowed any.
//!
//! # Why the counts and not just "it parses"
//!
//! An item that parses into three modifiers when the reference says six looks
//! exactly like one that parsed correctly. Six explicits read as one is a real
//! bug this project has had, and it passed the whole suite at the time.
//!
//! Porting these numbers found another: a body armour with one desecrated
//! modifier came back with five more, because the section's type was read
//! once and applied to every modifier in it.
//!
//! # What is not asserted, and why
//!
//! Not every number in the fixture file is a parser expectation. The reference
//! declares `UncutSkillGem.gemLevel = 19` as an input for its filter tests and
//! separately asserts that parsing that same item yields **no** gem level.
//! Harvesting every number and asserting it would have pinned the opposite of
//! what the reference says. Only the fields its own tests assert on parse
//! output are checked here.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use poe_trader_app::adapter::game_data_adapter::GameTables;
use poe_trader_core::controller::parse::parse_clipboard;
use poe_trader_core::controller::parse::sections::text_to_sections;
use poe_trader_core::types::item::ParsedItem;
use poe_trader_core::types::modifier::{Generation, ModifierType};
use poe_trader_core::types::GameVersion;

/// The six fixtures no upstream test ever uses.
///
/// Their declared numbers are unchecked by the reference itself and two of
/// them are demonstrably wrong: `NormalShield.itemLevel` says 82 and its own
/// raw text says `Item Level: 54`. Asserting those would pin our parser to
/// somebody's uncorrected notes.
///
/// Found by searching every `*.test.ts` for each fixture's name. 20 of the 26
/// are exercised; these are the rest.
const UNEXERCISED: [&str; 6] = [
    "FracturedItem",
    "FracturedItemNoModMarked",
    "ItemAllTheModifierTypes",
    "NewExplicitTypeDefinitions",
    "UnidentifiedBase",
    "UnidentifiedTier",
];

/// Fields the reference declares but never asserts on parse output.
///
/// `gemLevel` is the clear case. The reference declares
/// `UncutSkillGem.gemLevel = 19` as an input for its filter tests and then
/// asserts, in `skillGem.test.ts`, that parsing that same item yields no gem
/// level at all. `itemLevel` is declared wrong on at least one fixture.
const UNASSERTED: [&str; 2] = ["gemLevel", "itemLevel"];

/// One fixture: its text and whatever the reference declared about it.
struct Fixture {
    name: String,
    text: String,
    /// Every number the reference declares. Floats now: attack speed is 1.2
    /// and physical damage is 48.5, and reading only whole numbers silently
    /// dropped both.
    expected: BTreeMap<String, f64>,
    /// The level and attribute requirements, when it declares them.
    requires: Option<BTreeMap<String, i64>>,
}

fn fixtures() -> Vec<Fixture> {
    let raw = include_str!("fixtures/upstream_items.json");

    let parsed: serde_json::Map<String, serde_json::Value> =
        serde_json::from_str(raw).expect("the fixture file is valid JSON");

    parsed
        .into_iter()
        .map(|(name, item)| {
            let object = item.as_object().expect("each fixture is an object");

            Fixture {
                name,
                text: object
                    .get("rawText")
                    .and_then(serde_json::Value::as_str)
                    .expect("every fixture has raw text")
                    .to_string(),
                expected: object
                    .iter()
                    .filter(|(key, _)| !UNASSERTED.contains(&key.as_str()))
                    .filter_map(|(key, value)| Some((key.clone(), value.as_f64()?)))
                    .collect(),
                requires: object.get("requires").and_then(|r| {
                    Some(
                        r.as_object()?
                            .iter()
                            .filter_map(|(k, v)| Some((k.clone(), v.as_i64()?)))
                            .collect(),
                    )
                }),
            }
        })
        .collect()
}

/// Where the PoE2 data lives, if it has been built.
///
/// Returns nothing when it has not. `poe-trader-datagen` writes it and it is
/// not committed, so a fresh checkout has none and these tests report that
/// rather than failing on a missing file.
fn data() -> Option<GameTables> {
    for dir in ["data-poe2", "../data-poe2"] {
        let path = PathBuf::from(dir);

        if path.join("stats.ndjson").exists() {
            return GameTables::load(&path).ok();
        }
    }

    None
}

/// Run every fixture through the parser, or skip with a reason.
fn parsed() -> Vec<(Fixture, ParsedItem)> {
    let Some(tables) = data() else {
        eprintln!(
            "upstream_conformance: no data-poe2 directory. \
             Run poe-trader-datagen --game poe2 --out-dir data-poe2 first."
        );

        return Vec::new();
    };

    fixtures()
        .into_iter()
        .filter(|f| !UNEXERCISED.contains(&f.name.as_str()))
        .filter_map(|fixture| {
            let item = parse_clipboard(&fixture.text, GameVersion::Poe2, &tables).ok()?;

            Some((fixture, item))
        })
        .collect()
}

fn count_generation(item: &ParsedItem, generation: Generation) -> i64 {
    item.modifiers
        .iter()
        .filter(|m| m.info.generation == Some(generation))
        .count() as i64
}

fn count_kind(item: &ParsedItem, kind: ModifierType) -> i64 {
    item.modifiers
        .iter()
        .filter(|m| m.info.kind == Some(kind))
        .count() as i64
}

/// Whether a parsed number matches the reference's.
///
/// A tenth of a tolerance, not a half. Attack speed is quoted to two decimals
/// and 1.2 against 1.25 is a different weapon.
fn same(got: Option<f64>, want: f64) -> bool {
    got.is_some_and(|v| (v - want).abs() < 0.01)
}

// ---------------------------------------------------------------------------
// The harness itself
// ---------------------------------------------------------------------------

#[test]
fn the_whole_reference_fixture_set_is_present() {
    // 26 in the reference. A harvest that silently dropped half would make
    // every test below pass on what was left.
    assert_eq!(fixtures().len(), 26);
}

#[test]
fn every_fixture_carries_at_least_one_expected_number() {
    // A fixture with only raw text asserts nothing, which is the state this
    // file was written to get out of.
    for fixture in fixtures() {
        assert!(
            !fixture.expected.is_empty(),
            "{} carries no expected values",
            fixture.name
        );
    }
}

#[test]
fn every_fixture_parses() {
    let Some(tables) = data() else {
        return;
    };

    for fixture in fixtures() {
        assert!(
            parse_clipboard(&fixture.text, GameVersion::Poe2, &tables).is_ok(),
            "{} did not parse",
            fixture.name
        );
    }
}

// ---------------------------------------------------------------------------
// Sections
// ---------------------------------------------------------------------------

#[test]
fn every_section_count_matches_the_reference() {
    // The splitter is the first thing every stage depends on. One section too
    // many or too few moves every later stage onto the wrong lines.
    for fixture in fixtures() {
        if UNEXERCISED.contains(&fixture.name.as_str()) {
            continue;
        }

        let Some(want) = fixture.expected.get("sectionCount").map(|v| *v as i64) else {
            continue;
        };

        assert_eq!(
            text_to_sections(&fixture.text).len() as i64,
            want,
            "{}",
            fixture.name
        );
    }
}

// ---------------------------------------------------------------------------
// Item properties, which the reference asserts directly on parse output
// ---------------------------------------------------------------------------

#[test]
fn every_defence_value_matches_the_reference() {
    for (fixture, item) in parsed() {
        for (key, got) in [
            ("armourAR", item.armour.ar),
            ("armourEV", item.armour.ev),
            ("armourES", item.armour.es),
            ("armourBLOCK", item.armour.block),
        ] {
            let Some(&want) = fixture.expected.get(key) else {
                continue;
            };

            assert!(
                same(got, want),
                "{} {key}: got {got:?}, reference says {want}",
                fixture.name
            );
        }
    }
}

#[test]
fn every_weapon_value_matches_the_reference() {
    for (fixture, item) in parsed() {
        for (key, got) in [
            ("weaponPHYSICAL", item.weapon.physical),
            ("weaponELEMENTAL", item.weapon.elemental),
            ("weaponCRIT", item.weapon.crit),
            ("weaponRELOAD", item.weapon.reload),
        ] {
            let Some(&want) = fixture.expected.get(key) else {
                continue;
            };

            assert!(
                same(got, want),
                "{} {key}: got {got:?}, reference says {want}",
                fixture.name
            );
        }
    }
}

#[test]
fn every_quality_matches_the_reference() {
    for (fixture, item) in parsed() {
        let Some(want) = fixture.expected.get("quality").map(|v| *v as i64) else {
            continue;
        };

        assert_eq!(item.quality.map(i64::from), Some(want), "{}", fixture.name);
    }
}

#[test]
fn the_unasserted_fields_are_kept_out_of_the_expectations() {
    // A regression guard on this file rather than on the parser. Letting
    // gemLevel back in would pin the opposite of what the reference asserts.
    for fixture in fixtures() {
        for field in UNASSERTED {
            assert!(
                !fixture.expected.contains_key(field),
                "{} still carries {field}",
                fixture.name
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Modifier counts
//
// Exact when the data resolved every modifier. A modifier whose stat is not in
// the table is not recorded as a modifier, in the reference or here, and the
// reference tests against its own bundled data while these run against the
// live one. So an item with unknown modifiers is checked as an upper bound
// with the shortfall accounted for, rather than skipped.
// ---------------------------------------------------------------------------

fn check_count(fixture: &Fixture, item: &ParsedItem, key: &str, got: i64) {
    let Some(want) = fixture.expected.get(key).map(|v| *v as i64) else {
        return;
    };

    let unknown = item.unknown_modifiers.len() as i64;

    if unknown == 0 {
        assert_eq!(got, want, "{} {key}", fixture.name);

        return;
    }

    assert!(
        got <= want,
        "{} {key}: got {got}, more than the reference's {want}",
        fixture.name
    );

    assert!(
        want - got <= unknown,
        "{} {key}: {} missing but only {unknown} modifiers went unrecognised",
        fixture.name,
        want - got
    );
}

#[test]
fn every_prefix_count_matches_the_reference() {
    for (fixture, item) in parsed() {
        let got = count_generation(&item, Generation::Prefix);

        check_count(&fixture, &item, "prefixCount", got);
    }
}

#[test]
fn every_suffix_count_matches_the_reference() {
    for (fixture, item) in parsed() {
        let got = count_generation(&item, Generation::Suffix);

        check_count(&fixture, &item, "suffixCount", got);
    }
}

#[test]
fn every_implicit_count_matches_the_reference() {
    for (fixture, item) in parsed() {
        let got = count_kind(&item, ModifierType::Implicit);

        check_count(&fixture, &item, "implicitCount", got);
    }
}

#[test]
fn every_enchant_count_matches_the_reference() {
    for (fixture, item) in parsed() {
        let got = count_kind(&item, ModifierType::Enchant);

        check_count(&fixture, &item, "enchantCount", got);
    }
}

// ---------------------------------------------------------------------------
// What the reference asserts about modifier types
// ---------------------------------------------------------------------------

#[test]
fn a_desecrated_line_types_only_its_own_modifier() {
    // The bug porting these numbers found. A body armour with one modifier
    // ending in "(desecrated)" had all six read as desecrated, because the
    // section's type was read once and applied to every modifier in it. The
    // query then asked for six desecrated modifiers, which no item has.
    let Some(tables) = data() else {
        return;
    };

    let fixture = fixtures()
        .into_iter()
        .find(|f| f.name == "ArmourHighValueRareItem")
        .expect("the fixture is in the set");

    let item = parse_clipboard(&fixture.text, GameVersion::Poe2, &tables).expect("parses");

    let desecrated = count_kind(&item, ModifierType::Desecrated);

    assert_eq!(
        desecrated, 1,
        "the item has one desecrated line and {desecrated} desecrated modifiers"
    );
}

#[test]
fn an_uncut_gem_reports_no_gem_level() {
    // The reference asserts this outright. Its fixture declares
    // `gemLevel = 19` as an input for the filter tests, and the parser is
    // expected to leave the field empty because the game prints no level line.
    let Some(tables) = data() else {
        return;
    };

    for name in ["UncutSkillGem", "UncutSpiritGem", "UncutSupportGem"] {
        let Some(fixture) = fixtures().into_iter().find(|f| f.name == name) else {
            continue;
        };

        let item = parse_clipboard(&fixture.text, GameVersion::Poe2, &tables).expect("parses");

        assert_eq!(item.gem_level, None, "{name}");
    }
}

// ---------------------------------------------------------------------------
// The data file this runs against
// ---------------------------------------------------------------------------

#[test]
fn the_data_directory_is_reported_when_it_is_missing() {
    // A conformance suite that quietly passes because it found no data is
    // worse than one that fails. This states the condition so the reason a
    // run proved nothing is visible.
    if data().is_none() {
        eprintln!(
            "upstream_conformance: data-poe2 not found at {:?} or {:?}. \
             Every count test above proved nothing.",
            Path::new("data-poe2"),
            Path::new("../data-poe2")
        );
    }
}

// ---------------------------------------------------------------------------
// The map properties, from `renderer/specs/Parser/mapParser.test.ts`
//
// A waystone's worth is almost entirely these numbers. Pack size and rarity
// decide what the map drops, and a buyer filters on them rather than on the
// modifiers that granted them. Reading one into the wrong field is invisible:
// every number is plausible in every other field.
// ---------------------------------------------------------------------------

/// The reference's map fields, paired with what our parser calls them.
fn map_values(item: &ParsedItem) -> Vec<(&'static str, Option<f64>)> {
    vec![
        ("mapTier", item.map.tier.map(f64::from)),
        ("mapPackSize", item.map.pack_size),
        ("mapItemRarity", item.map.item_rarity),
        ("mapRevives", item.map.revives.map(f64::from)),
        ("mapDropChance", item.map.drop_chance),
        ("mapMagicMonsters", item.map.magic_monsters),
        ("mapRareMonsters", item.map.rare_monsters),
        ("mapMonsterRarity", item.map.monster_rarity),
        ("mapEffectiveness", item.map.effectiveness),
    ]
}

#[test]
fn every_map_number_matches_the_reference() {
    // Both of the reference's map fixtures, every field it declares.
    for (fixture, item) in parsed() {
        for (key, got) in map_values(&item) {
            let Some(&want) = fixture.expected.get(key) else {
                continue;
            };

            assert!(
                same(got, want),
                "{} {key}: got {got:?}, reference says {want}",
                fixture.name
            );
        }
    }
}

#[test]
fn a_map_with_every_property_reads_all_of_them() {
    // RareMapFakeAllProps exists in the reference precisely because a map
    // printing every line at once is where fields get crossed. Nine numbers,
    // all of them plausible in each other's slots.
    let Some(tables) = data() else {
        return;
    };

    let fixture = fixtures()
        .into_iter()
        .find(|f| f.name == "RareMapFakeAllProps")
        .expect("the fixture is in the set");

    let item = parse_clipboard(&fixture.text, GameVersion::Poe2, &tables).expect("parses");

    let declared = map_values(&item)
        .into_iter()
        .filter(|(key, _)| fixture.expected.contains_key(*key))
        .count();

    // The reference declares nine numbers for it. Reading fewer means a line
    // went unparsed and its value silently stayed absent.
    assert!(declared >= 8, "only {declared} map fields were declared");

    for (key, got) in map_values(&item) {
        if fixture.expected.contains_key(key) {
            assert!(got.is_some(), "{key} was declared and came back empty");
        }
    }
}

#[test]
fn a_map_tier_in_the_name_is_read() {
    // Both fixtures print the tier inside the base name, as
    // "Waystone (Tier 16)", and not on a line of its own.
    let Some(tables) = data() else {
        return;
    };

    for name in ["RareMap", "RareMapFakeAllProps"] {
        let fixture = fixtures()
            .into_iter()
            .find(|f| f.name == name)
            .expect("the fixture is in the set");

        let Some(want) = fixture.expected.get("mapTier").map(|v| *v as i64) else {
            continue;
        };

        let item = parse_clipboard(&fixture.text, GameVersion::Poe2, &tables).expect("parses");

        assert_eq!(item.map.tier.map(i64::from), Some(want), "{name}");
    }
}

#[test]
fn a_revive_count_of_zero_is_read_and_not_dropped() {
    // RareMapFakeAllProps declares zero revives. A parser that treated zero as
    // absent would lose the one number that makes the map harder, and the
    // failure is invisible because absent and zero look the same downstream.
    let Some(tables) = data() else {
        return;
    };

    let fixture = fixtures()
        .into_iter()
        .find(|f| f.name == "RareMapFakeAllProps")
        .expect("the fixture is in the set");

    assert_eq!(fixture.expected.get("mapRevives"), Some(&0.0));

    let item = parse_clipboard(&fixture.text, GameVersion::Poe2, &tables).expect("parses");

    assert_eq!(item.map.revives, Some(0));
}

// ---------------------------------------------------------------------------
// The fields the first harvest dropped
//
// It kept only whole numbers, so attack speed at 1.2, crit at 9.4 and physical
// damage at 48.5 were all thrown away before anything could compare them. The
// requirements were dropped too, being an object rather than a number.
//
// `integration.test.ts` asserts every one of these.
// ---------------------------------------------------------------------------

#[test]
fn every_fractional_weapon_number_matches_the_reference() {
    // Attack speed and crit are quoted to two decimals and are never whole.
    // Reading them with an integer parser truncates 1.2 to 1, which is a
    // different weapon and a very different price.
    // Collected rather than asserted one at a time. A bare assert stops at the
    // first mismatch and hides how many others there are, which turns one fix
    // into a dozen rounds of discovering the next.
    let mut wrong: Vec<String> = Vec::new();

    for (fixture, item) in parsed() {
        for (key, got) in [
            ("weaponAS", item.weapon.attack_speed),
            ("weaponCRIT", item.weapon.crit),
            ("weaponPHYSICAL", item.weapon.physical),
            ("weaponFIRE", item.weapon.fire),
            ("weaponCOLD", item.weapon.cold),
            ("weaponLIGHTNING", item.weapon.lightning),
            ("weaponSPIRIT", item.weapon.spirit),
        ] {
            let Some(&want) = fixture.expected.get(key) else {
                continue;
            };

            if !same(got, want) {
                wrong.push(format!(
                    "{} {key}: got {got:?}, reference says {want}",
                    fixture.name
                ));
            }
        }
    }

    assert!(
        wrong.is_empty(),
        "{} wrong:\n{}",
        wrong.len(),
        wrong.join("\n")
    );
}

#[test]
fn at_least_one_fixture_carries_a_fractional_number() {
    // A guard on the harvest rather than the parser. If the fixture ever loses
    // its floats again the test above passes by having nothing to check.
    let fractional = fixtures()
        .into_iter()
        .filter(|f| f.expected.values().any(|v| v.fract() != 0.0))
        .count();

    assert!(fractional >= 5, "only {fractional} fixtures carry a float");
}

#[test]
fn every_requirement_matches_the_reference() {
    // The level and attributes decide who can equip the item at all. A buyer
    // filtering for something they cannot use is the search working perfectly
    // and helping nobody.
    for (fixture, item) in parsed() {
        let Some(want) = &fixture.requires else {
            continue;
        };

        let got = item.requires.unwrap_or_default();

        for (key, got) in [
            ("level", got.level),
            ("str", got.str),
            ("dex", got.dex),
            ("int", got.int),
        ] {
            let Some(&want) = want.get(key) else {
                continue;
            };

            assert_eq!(i64::from(got), want, "{} requires.{key}", fixture.name);
        }
    }
}

#[test]
fn an_item_the_reference_gives_requirements_actually_has_some() {
    // Zero across the board would satisfy the comparison above for an item
    // whose requirement section never parsed.
    for (fixture, item) in parsed() {
        let Some(want) = &fixture.requires else {
            continue;
        };

        // The reference declares zero for attributes an item does not need, so
        // only the level is reliably non zero.
        if want.get("level").copied().unwrap_or(0) == 0 {
            continue;
        }

        assert!(
            item.requires.is_some(),
            "{} declares requirements and parsed none",
            fixture.name
        );
    }
}

#[test]
fn enough_fixtures_declare_requirements_to_prove_something() {
    let with_requires = fixtures()
        .into_iter()
        .filter(|f| f.requires.is_some())
        .count();

    assert!(
        with_requires >= 15,
        "only {with_requires} declare requirements"
    );
}
