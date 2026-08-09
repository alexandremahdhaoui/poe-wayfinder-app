//! Exiled Exchange 2's magic base type expectations, run against ours.
//!
//! Ported from `renderer/specs/Parser/magic-name.test.ts`. It has twelve
//! cases: four in English and eight in Traditional Chinese. The Chinese ones
//! are not ported, because this build is English only by policy and has no
//! Chinese data file to run them against.
//!
//! # Why finding a magic item's base is hard
//!
//! A magic item's name is its base with a prefix, a suffix, or both glued on,
//! and nothing in the text marks where the affixes end. `Pulsing Antler Focus`
//! could be a base called `Pulsing Antler Focus`, one called `Antler Focus`,
//! or one called `Focus`. Only the item table knows.
//!
//! Getting it wrong means the query searches for a base that does not exist,
//! which returns nothing and reads as "this item is worthless".
//!
//! # Why the app crate
//!
//! It needs the real item table, and loading one is I/O.

use std::path::PathBuf;

use poe_wayfinder_app::adapter::game_data_adapter::GameTables;
use poe_wayfinder_core::controller::parse::magic_name::magic_base_type;
use poe_wayfinder_core::types::GameVersion;

/// The reference's four English cases, name and expected base.
const CASES: [(&str, &str); 4] = [
    // No affixes at all. The whole name is the base.
    ("Rattling Sceptre", "Rattling Sceptre"),
    // A suffix only.
    ("Cultist Greathammer of Nourishment", "Cultist Greathammer"),
    // A prefix only. Note the base itself is two words, so a parser that
    // dropped one leading word would answer `Focus` and find nothing.
    ("Pulsing Antler Focus", "Antler Focus"),
    // Both ends at once.
    ("Reaver's Temple Maul of Stunning", "Temple Maul"),
];

fn data() -> Option<GameTables> {
    for dir in ["data-poe2", "../data-poe2"] {
        let path = PathBuf::from(dir);

        if path.join("items.ndjson").exists() {
            return GameTables::load(&path).ok();
        }
    }

    None
}

#[test]
fn every_english_case_the_reference_lists_resolves() {
    let Some(tables) = data() else {
        eprintln!("upstream_magic_name: no data-poe2 directory, nothing proved");

        return;
    };

    for (name, expected) in CASES {
        let got = magic_base_type(name, &tables, GameVersion::Poe2);

        assert_eq!(got.as_deref(), Some(expected), "{name}");
    }
}

#[test]
fn a_base_with_no_affixes_is_returned_whole() {
    // The reference's first case. Stripping a word here would turn a valid
    // base into one that does not exist.
    let Some(tables) = data() else {
        return;
    };

    assert_eq!(
        magic_base_type("Rattling Sceptre", &tables, GameVersion::Poe2).as_deref(),
        Some("Rattling Sceptre")
    );
}

#[test]
fn a_two_word_base_keeps_both_words() {
    // `Pulsing Antler Focus` is `Antler Focus` and not `Focus`. Taking the
    // shortest match that resolves would answer `Focus`, search for a base
    // nobody sells, and report the item as worthless.
    let Some(tables) = data() else {
        return;
    };

    let got = magic_base_type("Pulsing Antler Focus", &tables, GameVersion::Poe2);

    assert_eq!(got.as_deref(), Some("Antler Focus"));
    assert_ne!(got.as_deref(), Some("Focus"));
}

#[test]
fn affixes_at_both_ends_come_off_together() {
    let Some(tables) = data() else {
        return;
    };

    assert_eq!(
        magic_base_type(
            "Reaver's Temple Maul of Stunning",
            &tables,
            GameVersion::Poe2
        )
        .as_deref(),
        Some("Temple Maul")
    );
}

#[test]
fn a_name_that_is_no_base_at_all_resolves_to_nothing() {
    // Better than guessing. A wrong base searches for something that does not
    // exist, and the empty result reads as a price of zero.
    let Some(tables) = data() else {
        return;
    };

    assert_eq!(
        magic_base_type("Nonsense Words Of Nothing", &tables, GameVersion::Poe2),
        None
    );
}

#[test]
fn the_longest_base_wins_over_a_shorter_one_inside_it() {
    // The invariant behind the two word case, stated on its own. Every case
    // the reference lists depends on it.
    let Some(tables) = data() else {
        return;
    };

    for (name, expected) in CASES {
        let Some(got) = magic_base_type(name, &tables, GameVersion::Poe2) else {
            panic!("{name} resolved to nothing");
        };

        assert!(
            got.split_whitespace().count() >= expected.split_whitespace().count(),
            "{name} resolved to {got}, shorter than the reference's {expected}"
        );
    }
}
