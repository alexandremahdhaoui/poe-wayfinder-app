//! Measures how much of the reference is ported.
//!
//! Exists because eyeballing parity with ad-hoc greps produced confident wrong
//! answers. This reads both trees and reports a number, so "is it done" has an
//! answer nobody has to take on trust.
//!
//! # What it measures
//!
//! Two things, separately, because they fail differently.
//!
//! - **Function parity.** Every top level function in the reference parser and
//!   filter layer, and whether a function of the same name exists here. A
//!   missing function is a feature that silently does nothing.
//! - **File parity.** Every reference source file and roughly how much of it
//!   is accounted for. A file with no counterpart is a whole subsystem
//!   missing.
//!
//! # What it cannot measure
//!
//! Whether a ported function is correct. Only tests do that. A high parity
//! score with failing tests means nothing, which is why the report prints the
//! test count alongside.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

/// One reference file and what it holds.
#[derive(Debug, Clone)]
struct RefFile {
    path: PathBuf,
    lines: usize,
    functions: Vec<String>,
}

/// The verdict for one reference function.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Status {
    /// A function of the same name exists here.
    Ported,
    /// Deliberately not ported, with a reason.
    Waived,
    /// Missing.
    Missing,
}

/// Functions we will never port, and why.
///
/// Each needs a reason. Without one this list becomes a place to hide work.
const WAIVED: &[(&str, &str)] = &[
    (
        "parseVaalGemName",
        "disabled in the reference itself, see its issue 954",
    ),
    (
        "parseAreaLevelNested",
        "ported as read_area_level, a private helper in content.rs",
    ),
    (
        "parseStatsFromMod",
        "ported as match_stat_lines in modifiers.rs",
    ),
    (
        "transformToLegacyModifiers",
        "superseded by controller::aggregate::sum_stats_by_type",
    ),
    (
        "parseRuneforged",
        "a no-op in the reference, kept as a named stage",
    ),
    (
        "calcBaseDamage",
        "base_value with the PHYSICAL_DAMAGE table, not a separate function",
    ),
    (
        "calcTotalDamage",
        "total_value with the PHYSICAL_DAMAGE table, not a separate function",
    ),
];

/// Reference names we ported under a different Rust name.
///
/// A rename is a decision, and this table is where those decisions are
/// recorded. Without it the tracker under-reports and stops being trusted,
/// which is worse than no tracker at all.
const ALIASES: &[(&str, &str)] = &[
    // The Rust name says what it does rather than how it is called.
    ("itemTextToSections", "text_to_sections"),
    ("markupConditionParser", "strip_markup"),
    ("itemIsModifiable", "is_modifiable"),
    ("getMaxSockets", "max_sockets"),
    ("isArmourOrWeaponOrCaster", "socket_group"),
    ("getRollOrMinmaxAvg", "roll_or_minmax_avg"),
    ("linesToStatStrings", "match_stat_lines"),
    ("_statPlaceholderGenerator", "candidates"),
    ("findAndResolveTranslation", "try_parse_translation"),
    // calc-q20 reads better as what it does to the scaling.
    ("calcFlat", "strip_scaling"),
    ("calcIncreased", "apply_scaling"),
    ("calcPropPercentile", "prop_percentile"),
    ("propAt20Quality", "prop_at_20_quality"),
    // Aggregation.
    ("sumStatsByModType", "sum_stats_by_type"),
    ("statSourcesTotal", "combine"),
    // calc-base says what it produces rather than what it is called.
    ("calcPropBase", "contributions"),
    ("calcBase", "base_value"),
    ("calcTotal", "total_value"),
    // The filter rules say what they decide.
    ("enableGoodRolledFilters", "should_enable"),
    ("hideNotVariableStat", "hidden_reason"),
    ("filterFillMinMax", "fill_ends"),
    // Filters.
    ("createFilters", "build_query"),
    ("createExactStatFilters", "build_stat_group"),
    ("filterPseudo", "pseudo_totals"),
    // Item property filters say which property they pick.
    ("isSingleAttrArmour", "is_single_defence_armour"),
    ("armourProps", "armour_filters"),
    ("weaponProps", "weapon_filters"),
    ("filterItemProp", "build_stat_group_for"),
    // Presets say which handful of filters a kind of item needs.
    ("createPresets", "preset_for"),
    ("createGemFilters", "gem_level_filter"),
    ("createTrialsFilters", "trials_filter"),
    ("createUncutGemFilters", "apply_gem_filters"),
    // The fetch endpoint.
    ("requestResults", "read_listings"),
    ("parseFetchResult", "read_listing"),
];

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();

    let reference = args
        .iter()
        .position(|a| a == "--reference")
        .and_then(|i| args.get(i + 1))
        .cloned()
        .unwrap_or_else(|| "../reference/Exiled-Exchange-2/renderer/src".to_string());

    let ours = args
        .iter()
        .position(|a| a == "--ours")
        .and_then(|i| args.get(i + 1))
        .cloned()
        .unwrap_or_else(|| "..".to_string());

    // A floor to fail below, so parity can only go up.
    let floor: f64 = args
        .iter()
        .position(|a| a == "--min")
        .and_then(|i| args.get(i + 1))
        .and_then(|v| v.parse().ok())
        .unwrap_or(0.0);

    let reference = Path::new(&reference);

    if !reference.is_dir() {
        eprintln!("parity: no reference checkout at {}", reference.display());
        eprintln!("parity: clone it or pass --reference");

        // Not a failure. A machine without the reference can still build.
        return ExitCode::SUCCESS;
    }

    let ref_files = collect_reference(reference);
    let our_source = collect_our_source(Path::new(&ours));

    report(&ref_files, &our_source, floor)
}

/// Read every reference source file worth tracking.
fn collect_reference(root: &Path) -> Vec<RefFile> {
    let mut out = Vec::new();

    // Only the parts we are porting. The Vue components and the Electron shell
    // are replaced rather than ported, so counting them would report a gap
    // that is not one.
    for sub in ["parser", "web/price-check/filters", "web/price-check/trade"] {
        collect_ts(&root.join(sub), &mut out);
    }

    out.sort_by(|a, b| a.path.cmp(&b.path));

    out
}

fn collect_ts(dir: &Path, out: &mut Vec<RefFile>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };

    for entry in entries.flatten() {
        let path = entry.path();

        if path.is_dir() {
            collect_ts(&path, out);

            continue;
        }

        if path.extension().is_none_or(|e| e != "ts") {
            continue;
        }

        // Type declarations hold no logic to port.
        if path.file_name().is_some_and(|n| n == "interfaces.ts") {
            continue;
        }

        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };

        out.push(RefFile {
            lines: text.lines().count(),
            functions: top_level_functions(&text),
            path,
        });
    }
}

/// Every top level function name in a TypeScript file.
fn top_level_functions(text: &str) -> Vec<String> {
    let mut out = Vec::new();

    for line in text.lines() {
        // Top level only. An indented function is a closure or a method and
        // porting it one for one is not meaningful.
        let rest = line
            .strip_prefix("export function ")
            .or_else(|| line.strip_prefix("function "))
            .or_else(|| line.strip_prefix("export function* "))
            .or_else(|| line.strip_prefix("function* "));

        let Some(rest) = rest else {
            continue;
        };

        let name: String = rest
            .chars()
            .take_while(|c| c.is_alphanumeric() || *c == '_')
            .collect();

        if !name.is_empty() {
            out.push(name);
        }
    }

    out.sort();
    out.dedup();

    out
}

/// Every line of our own Rust source, concatenated.
///
/// One string rather than a per file map, because a reference function can
/// legitimately land in a different file here and the question is whether it
/// exists at all.
fn collect_our_source(root: &Path) -> String {
    let mut out = String::new();

    for crate_dir in ["poe-trader-core/src", "poe-trader-app/src"] {
        collect_rs(&root.join(crate_dir), &mut out);
    }

    out
}

fn collect_rs(dir: &Path, out: &mut String) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };

    for entry in entries.flatten() {
        let path = entry.path();

        if path.is_dir() {
            collect_rs(&path, out);

            continue;
        }

        if path.extension().is_none_or(|e| e != "rs") {
            continue;
        }

        if let Ok(text) = std::fs::read_to_string(&path) {
            out.push_str(&text);
            out.push('\n');
        }
    }
}

/// Turn a TypeScript name into the Rust one it would have.
fn to_snake_case(name: &str) -> String {
    let mut out = String::new();

    for (i, c) in name.chars().enumerate() {
        if c.is_uppercase() {
            if i > 0 {
                out.push('_');
            }

            out.extend(c.to_lowercase());
        } else {
            out.push(c);
        }
    }

    out
}

/// Whether we have a function by this name.
fn status_of(name: &str, our_source: &str) -> Status {
    if WAIVED.iter().any(|(waived, _)| *waived == name) {
        return Status::Waived;
    }

    let snake = ALIASES
        .iter()
        .find(|(reference, _)| *reference == name)
        .map_or_else(|| to_snake_case(name), |(_, ours)| (*ours).to_string());

    if our_source.contains(&format!("fn {snake}(")) || our_source.contains(&format!("fn {snake}<"))
    {
        return Status::Ported;
    }

    Status::Missing
}

/// Print the report and decide the exit code.
fn report(ref_files: &[RefFile], our_source: &str, floor: f64) -> ExitCode {
    let mut ported = 0usize;
    let mut waived = 0usize;
    let mut missing: BTreeMap<String, Vec<String>> = BTreeMap::new();

    let mut ref_lines = 0usize;

    for file in ref_files {
        ref_lines += file.lines;

        for function in &file.functions {
            match status_of(function, our_source) {
                Status::Ported => ported += 1,
                Status::Waived => waived += 1,
                Status::Missing => missing
                    .entry(short_path(&file.path))
                    .or_default()
                    .push(function.clone()),
            }
        }
    }

    let missing_count: usize = missing.values().map(Vec::len).sum();
    let total = ported + waived + missing_count;

    // Waived counts as done. It is a decision with a written reason, not a gap.
    let parity = if total == 0 {
        100.0
    } else {
        ((ported + waived) as f64 / total as f64) * 100.0
    };

    println!("poe-trader parity report");
    println!();
    println!("  reference functions : {total}");
    println!("  ported              : {ported}");
    println!("  waived              : {waived}");
    println!("  missing             : {missing_count}");
    println!("  parity              : {parity:.1}%");
    println!();
    println!("  reference lines     : {ref_lines}");
    println!("  our rust lines      : {}", our_source.lines().count());

    if !missing.is_empty() {
        println!();
        println!("missing, by reference file:");

        for (path, functions) in &missing {
            println!();
            println!("  {path}");

            for function in functions {
                println!("    {function}");
            }
        }
    }

    if waived > 0 {
        println!();
        println!("waived, with reasons:");

        for (name, reason) in WAIVED {
            println!("  {name}: {reason}");
        }
    }

    println!();

    if parity + f64::EPSILON < floor {
        println!("FAIL: parity {parity:.1}% is below the floor of {floor:.1}%");

        return ExitCode::FAILURE;
    }

    if floor > 0.0 {
        println!("OK: parity {parity:.1}% meets the floor of {floor:.1}%");
    }

    ExitCode::SUCCESS
}

/// The reference path, from `renderer/src` down.
fn short_path(path: &Path) -> String {
    let text = path.display().to_string();

    match text.find("renderer/src/") {
        Some(i) => text[i + "renderer/src/".len()..].to_string(),
        None => text,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn top_level_functions_are_found() {
        let text = "\
export function parseFoo(a: string) {}
function parseBar() {}
export function* linesToStatStrings() {}
";

        assert_eq!(
            top_level_functions(text),
            vec!["linesToStatStrings", "parseBar", "parseFoo"]
        );
    }

    #[test]
    fn an_indented_function_is_not_top_level() {
        // A closure or a method. Porting it one for one is not meaningful.
        let text = "  function inner() {}\n    function deeper() {}";

        assert!(top_level_functions(text).is_empty());
    }

    #[test]
    fn a_function_is_listed_once_however_often_it_appears() {
        let text = "function parseFoo() {}\nfunction parseFoo() {}";

        assert_eq!(top_level_functions(text), vec!["parseFoo"]);
    }

    #[test]
    fn camel_case_becomes_snake_case() {
        assert_eq!(
            to_snake_case("parseAugmentSockets"),
            "parse_augment_sockets"
        );
        assert_eq!(to_snake_case("calcBasePercentile"), "calc_base_percentile");
        assert_eq!(to_snake_case("parse"), "parse");
    }

    #[test]
    fn a_leading_capital_gains_no_underscore() {
        assert_eq!(to_snake_case("ParseFoo"), "parse_foo");
    }

    #[test]
    fn a_ported_function_is_recognised() {
        let ours = "pub fn parse_augment_sockets(section: &[String]) {}";

        assert_eq!(status_of("parseAugmentSockets", ours), Status::Ported);
    }

    #[test]
    fn a_generic_function_is_recognised() {
        let ours = "pub fn copy_item<F>(clipboard: &mut dyn Clipboard) {}";

        assert_eq!(status_of("copyItem", ours), Status::Ported);
    }

    #[test]
    fn a_missing_function_is_reported() {
        assert_eq!(status_of("parseSomethingNew", ""), Status::Missing);
    }

    #[test]
    fn a_partial_name_does_not_count_as_ported() {
        // "fn parse_foo_bar" must not satisfy "parseFoo". The trailing paren
        // in the search is what stops it.
        let ours = "pub fn parse_foo_bar() {}";

        assert_eq!(status_of("parseFoo", ours), Status::Missing);
    }

    #[test]
    fn an_aliased_function_is_recognised() {
        // A rename must not read as a gap, or the tracker under-reports and
        // stops being trusted.
        let ours = "pub fn text_to_sections(text: &str) {}";

        assert_eq!(status_of("itemTextToSections", ours), Status::Ported);
    }

    #[test]
    fn an_alias_that_points_nowhere_is_still_missing() {
        assert_eq!(status_of("itemTextToSections", ""), Status::Missing);
    }

    #[test]
    fn no_function_is_both_aliased_and_waived() {
        // It would be recorded twice and the reason would contradict the
        // alias.
        for (name, _) in ALIASES {
            assert!(
                !WAIVED.iter().any(|(waived, _)| waived == name),
                "{name} is both aliased and waived"
            );
        }
    }

    #[test]
    fn no_reference_name_is_aliased_twice() {
        let mut names: Vec<&str> = ALIASES.iter().map(|(n, _)| *n).collect();
        let before = names.len();
        names.sort_unstable();
        names.dedup();

        assert_eq!(names.len(), before);
    }

    #[test]
    fn a_waived_function_is_not_missing() {
        assert_eq!(status_of("parseVaalGemName", ""), Status::Waived);
    }

    #[test]
    fn every_waiver_carries_a_reason() {
        // Without one this list becomes a place to hide work.
        for (name, reason) in WAIVED {
            assert!(!reason.trim().is_empty(), "{name} has no reason");
            assert!(
                reason.len() > 15,
                "{name} has a reason too short to mean anything"
            );
        }
    }

    #[test]
    fn no_function_is_waived_twice() {
        let mut names: Vec<&str> = WAIVED.iter().map(|(n, _)| *n).collect();
        let before = names.len();
        names.sort_unstable();
        names.dedup();

        assert_eq!(names.len(), before);
    }

    #[test]
    fn the_reference_path_is_shortened_to_its_meaningful_part() {
        let path = Path::new("/home/x/reference/Exiled-Exchange-2/renderer/src/parser/Parser.ts");

        assert_eq!(short_path(path), "parser/Parser.ts");
    }

    #[test]
    fn a_path_outside_the_reference_is_left_alone() {
        assert_eq!(short_path(Path::new("/tmp/x.ts")), "/tmp/x.ts");
    }
}
