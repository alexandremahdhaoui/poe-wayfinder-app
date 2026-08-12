use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

#[derive(Debug, Clone)]
struct RefFile {
    path: PathBuf,
    lines: usize,
    functions: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Status {
    Ported,
    Waived,
    Missing,
}

const WAIVED: &[(&str, &str)] = &[
    (
        "checkForUpdates",
        "Electron's autoUpdater. This ships one unsigned exe with no update channel, and its game data refreshes itself instead.",
    ),
    (
        "openDownloadPage",
        "opens the reference's release page, part of the same updater",
    ),
    (
        "quitAndInstall",
        "restarts into a downloaded update, part of the same updater",
    ),
    (
        "configModelValue",
        "a Vue two way binding onto the config object. Ours reads the generated config struct directly.",
    ),
    ("_configModelValue", "the private half of configModelValue, same reason"),
    ("configProp", "declares a Vue prop bound to a config key, same reason"),
    (
        "quit",
        "closes the Electron app from its own menu. Ours quits from the tray and the launcher, neither of which is a function by that name.",
    ),
    (
        "displayRounding",
        "rounds a poe.ninja price for display, and poe.ninja is a third party this workspace forbids",
    ),
    (
        "getAvailableCoreCurrencies",
        "reads the currency list from poe.ninja, same reason",
    ),
    ("parseXchg", "parses a poe.ninja exchange blob, same reason"),
    ("splitJsonBlob", "splits a poe.ninja response, same reason"),
    (
        "btnStyle",
        "returns Vue class names for the map mod button. Ours picks a Color32 inline in the tab.",
    ),
    (
        "newStatIconVisible",
        "shows a new badge against the reference's own data version, which our flat data file has no counterpart for",
    ),
    (
        "findAllAreaMods",
        "walks the reference's map mod table to offer every mod for marking. Ours marks what the item actually rolled.",
    ),
    (
        "tagToShowOrder",
        "sorts the settings list by the reference's stat tags, which our data does not carry",
    ),
    (
        "fuzzyFindHeistGem",
        "fuzzy matches a heist gem name, and heist is PoE1 content the PoE2 reference dropped",
    ),
    (
        "makeInvisible",
        "a Vue transition that fades a row out, with no equivalent in an immediate mode panel",
    ),
    (
        "diffItem",
        "compares two library rows to colour what changed between them, a Vue table concern",
    ),
    ("modFilter", "filters the CSV columns from a Vue checkbox list"),
    ("modToShortMod", "shortens a mod for a CSV column header, part of the same Vue export dialog"),
    ("flatJoin", "joins Vue menu entries for rendering"),
    ("menuByType", "groups the settings menu by widget type, which our tabs do by hand"),
    ("shuffle", "shuffles the settings menu order for the reference's random tip banner"),
    (
        "useMarketRatioFinder",
        "reads the going rate from poe.ninja, a third party this workspace forbids",
    ),
    (
        "mergeWithMarketRatio",
        "inserts the poe.ninja rate into the listings, so it goes with useMarketRatioFinder",
    ),
    (
        "noSourcePseudoToFilter",
        "the pseudo loop in stat_filters.rs builds these inline, not through a named helper",
    ),
    (
        "findAndResolveByRef",
        "our data file is flat, one record per printed text, so there is no group to resolve: stat_by_matcher is the whole lookup",
    ),
    (
        "_findAndResolveByRef",
        "the private half of findAndResolveByRef, same reason",
    ),
    (
        "_resolveTranslation",
        "picks one stat out of a translation group. Our builder merges a group into one record and the several trade ids travel as a count group, which matches whichever the listing used",
    ),
    (
        "testItemCategory",
        "only used by _resolveTranslation's select strategy, which our flat data has no group to run on",
    ),
    (
        "parseMemoryStrandsNested",
        "the strand line loop inside parse_accessory, not a separate function",
    ),
    (
        "parseScryingOrb",
        "reads a map area from the AREA table, which the trade API does not publish: the reference extracts it from the game bundles",
    ),
    (
        "shortcutToElectron",
        "maps a hotkey to Electron accelerator syntax, which this build has no Electron to accept",
    ),
    (
        "addFileUploadRoutes",
        "an Electron HTTP file upload server, deliberately absent: this build opens no listening socket",
    ),
    (
        "artificialSlowdown",
        "a Vue reactive timer for the spinner, no equivalent in an egui overlay",
    ),
    (
        "useTradeApi",
        "a Vue composable wrapper, replaced by trade_api_adapter in poe-wayfinder-app",
    ),
    (
        "useBulkApi",
        "a Vue composable wrapper, replaced by trade_api_adapter in poe-wayfinder-app",
    ),
    (
        "t",
        "the vue-i18n translate binding, this build is English only by policy",
    ),
    (
        "filterPseudoSources",
        "a flat_map over sources inline in pseudo_totals, not a named helper",
    ),
    (
        "augmentCount",
        "returns a hardcoded 1 in the reference, its real body is commented out",
    ),
    (
        "propToFilter",
        "a struct literal on PropertyFilter, not a function, in item_property.rs",
    ),
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

const ALIASES: &[(&str, &str)] = &[
    ("fmtTime", "clock"),
    ("findWidget", "find_widget"),
    ("enableWidget", "enable_widget"),
    ("disableWidget", "disable_widget"),
    ("decisionCreate", "set_verdict"),
    ("decisionHasColor", "is_coloured"),
    ("nextDecision", "next"),
    ("isOutdated", "is_outdated"),
    ("prepareMapStats", "review"),
    ("handleClick", "cycle_verdict"),
    ("toggleSeenStatus", "cycle_verdict"),
    ("openWiki", "wiki"),
    ("openPoedb", "poedb"),
    ("openCoE", "craft_of_exile"),
    ("getPoe2dbPath", "poedb"),
    ("encodePoe2dbUri", "encode"),
    ("registerActions", "open_link"),
    ("findSimilarItems", "similar_items"),
    ("findSamePricedItems", "same_priced"),
    ("findByPrice", "same_priced"),
    ("findItems", "search"),
    ("selectItem", "star"),
    ("clearSelectedItems", "clear_stars"),
    ("useSelectedItems", "starred"),
    ("starredItemClick", "toggle_star"),
    ("buildCsvString", "to_csv"),
    ("arrayToCsvString", "csv_line"),
    ("getHeader", "header"),
    ("startSessionHost", "record"),
    ("endSessionHost", "clear"),
    ("getExpPenalty", "penalty"),
    ("calcBaseSafeZone", "safe_zone"),
    ("getOverIdeal", "effective_difference"),
    ("isPrivateLeague", "is_private_league"),
    ("randomTip", "tip"),
    ("parseClientLogText", "parse_log_text"),
    ("parseLogVersion0", "parse_log_line"),
    ("lessThanVersion", "older_than"),
    ("getClientLogParseVersion", "parse_version"),
    ("useRemovable", "dismiss"),
    ("toggle", "set_enabled"),
    ("isPoeItem", "clipboard_kind"),
    ("isPointInsideRect", "point_in_rect"),
    ("isStashArea", "is_stash_area"),
    ("eventToString", "event_to_hotkey"),
    ("pressKeysToCopyItemText", "keys_to_hold_for_copy"),
    ("typeInChat", "type_in_chat"),
    ("stashSearch", "stash_search"),
    ("parseConfigHotkey", "parse_config_hotkey"),
    ("readConfig", "parse_ini"),
    ("finalFilterTweaks", "final_filter_tweaks"),
    ("createNewStatFilter", "preview_filters"),
    ("translateStatWithRoll", "wording_for"),
    ("buildMageBloodNotFilter", "duplicates_filter"),
    ("buildFilterWithValue", "duplicates_filter"),
    ("calcPropBounds", "prop_bounds"),
    (
        "trySecondaryParseTranslation",
        "try_secondary_parse_translation",
    ),
    ("createVirtualItem", "virtual_item"),
    ("applyContractRules", "contract_filters"),
    ("applyBlueprintRules", "blueprint_exclusion"),
    ("applyFlaskHybridMod", "flask_excludes_increased_effect"),
    ("filterMemoryStrands", "memory_strands_filter"),
    ("statToNotFilter", "not_group"),
    ("_mergeTradeIdsInto", "merge_trade_ids_into"),
    ("calculatedStatToFilter", "build_one"),
    ("initUiModFilters", "build_stat_group"),
    ("createTradeRequest", "build_query"),
    ("nameToQuery", "new"),
    ("tradeIdToQuery", "stat_filter_to_json"),
    ("parseMods", "mod_block"),
    ("applyEleAugment", "apply_elemental_rune"),
    ("recalculateItemProperties", "rescale"),
    ("refEffectsPseudos", "affects_pseudo"),
    ("translatedEffectsPseudos", "signs_match"),
    ("shortRollToFilter", "short_roll_to_filter"),
    ("filterAdjustmentForNegate", "negate"),
    ("getMinMax", "for_trade"),
    ("parseModifiersPoe2", "read_modifier_section_poe2"),
    ("mapProps", "map_filters"),
    ("filterBasePercentile", "base_percentile_filter"),
    ("removeUsedStats", "remove_used_stats"),
    ("apiToSatisfySearch", "endpoint_for"),
    ("tradeTag", "trade_tag"),
    ("preventQueueCreation", "queue_wait"),
    ("toPricingResult", "seller_status"),
    ("adjustRateLimits", "adjust"),
    ("_adjustRateLimits", "parse_rate_limit_headers"),
    ("decimalPlaces", "decimal_places"),
    ("roundRoll", "round_roll"),
    ("percentRoll", "percent_roll"),
    ("percentRollDelta", "percent_roll_delta"),
    ("getItemEditorType", "editor_kind"),
    ("parseAffixStrings", "parse_affix_strings"),
    ("getTier", "tier_at"),
    ("getTierV2", "tier_of"),
    ("parseModBlock", "mod_block"),
    ("buildItemProps", "item_properties"),
    ("buildGrantSkillBlock", "granted_skills"),
    ("buildNameBlock", "item_tags"),
    ("magicBasetype", "magic_base_type"),
    ("replaceHashWithValues", "fill_placeholders"),
    ("modsEqual", "mods_equal"),
    ("applyIncr", "apply_incr"),
    ("maxUsefulItemLevel", "max_useful_item_level"),
    ("enableAllFilters", "enable_all"),
    ("selectAugmentEffectByItemCategory", "effect_for_category"),
    ("getAugmentNameByRef", "augment_name"),
    ("handleApplyItemEdits", "apply"),
    ("handleRemoveItemEdits", "remove"),
    ("itemTextToSections", "text_to_sections"),
    ("markupConditionParser", "strip_markup"),
    ("itemIsModifiable", "is_modifiable"),
    ("getMaxSockets", "max_sockets"),
    ("isArmourOrWeaponOrCaster", "socket_group"),
    ("getRollOrMinmaxAvg", "roll_or_minmax_avg"),
    ("linesToStatStrings", "match_stat_lines"),
    ("_statPlaceholderGenerator", "candidates"),
    ("findAndResolveTranslation", "try_parse_translation"),
    ("calcFlat", "strip_scaling"),
    ("calcIncreased", "apply_scaling"),
    ("calcPropPercentile", "prop_percentile"),
    ("propAt20Quality", "prop_at_20_quality"),
    ("sumStatsByModType", "sum_stats_by_type"),
    ("statSourcesTotal", "combine"),
    ("calcPropBase", "contributions"),
    ("calcBase", "base_value"),
    ("calcTotal", "total_value"),
    ("enableGoodRolledFilters", "should_enable"),
    ("hideNotVariableStat", "hidden_reason"),
    ("filterFillMinMax", "fill_ends"),
    ("createFilters", "build_query"),
    ("createExactStatFilters", "build_stat_group"),
    ("filterPseudo", "pseudo_totals"),
    ("isSingleAttrArmour", "is_single_defence_armour"),
    ("armourProps", "armour_filters"),
    ("weaponProps", "weapon_filters"),
    ("filterItemProp", "build_stat_group_for"),
    ("createPresets", "preset_for"),
    ("createGemFilters", "gem_level_filter"),
    ("createTrialsFilters", "trials_filter"),
    ("createUncutGemFilters", "apply_gem_filters"),
    ("requestResults", "read_listings"),
    ("parseFetchResult", "read_listing"),
    ("createUniquePresets", "unique_search"),
    ("createMagebloodFilters", "link_filter"),
    ("applyAnointmentRules", "anointment"),
    ("decodeOils", "anointment"),
    ("applyRules", "valuable_rooms"),
    ("explicitModifierCount", "explicit_modifier_count"),
    ("itemBaseMaxModifiersOfType", "max_modifiers_of_type"),
    ("itemMaxModifiersBySlot", "max_modifiers_of_type"),
    ("showHasEmptyModifier", "empty_slot"),
    ("likelyFinishedItem", "likely_finished"),
    ("applyClusterJewelRules", "passive_bound"),
    ("applyFlaskRules", "flask_enchant_is_useful"),
    ("hideAllAugments", "is_hidden_by_default"),
    ("hasCraftingValue", "empty_slot"),
    ("isItemMissingItemClass", "is_missing_item_class"),
    ("floorToBracket", "floor_to_bracket"),
    ("ceilToBracket", "ceil_to_bracket"),
    (
        "areaLevelByAscendancyPoints",
        "area_level_by_ascendancy_points",
    ),
    (
        "ascendancyPointsByAreaLevel",
        "ascendancy_points_by_area_level",
    ),
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

    let subdirs: Vec<String> = args
        .iter()
        .position(|a| a == "--subdirs")
        .and_then(|i| args.get(i + 1))
        .map_or_else(
            || {
                ["parser", "web/price-check/filters", "web/price-check/trade"]
                    .iter()
                    .map(|s| (*s).to_string())
                    .collect()
            },
            |v| v.split(',').map(|s| s.trim().to_string()).collect(),
        );

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

        return ExitCode::SUCCESS;
    }

    let ref_files = collect_reference(reference, &subdirs);
    let our_source = collect_our_source(Path::new(&ours));

    report(&ref_files, &our_source, floor)
}

fn script_block(text: &str) -> String {
    let mut out = String::new();
    let mut rest = text;

    while let Some(open) = rest.find("<script") {
        let after = &rest[open..];

        let Some(body) = after.find('>').map(|i| &after[i + 1..]) else {
            break;
        };

        let Some(close) = body.find("</script>") else {
            out.push_str(body);

            break;
        };

        out.push_str(&body[..close]);
        out.push('\n');

        rest = &body[close + "</script>".len()..];
    }

    out
}

fn collect_reference(root: &Path, subdirs: &[String]) -> Vec<RefFile> {
    let mut out = Vec::new();

    for sub in subdirs {
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

        let readable = path.extension().is_some_and(|e| e == "ts" || e == "vue");

        if !readable {
            continue;
        }

        if path.file_name().is_some_and(|n| n == "interfaces.ts") {
            continue;
        }

        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };

        let source = match path.extension().is_some_and(|e| e == "vue") {
            true => script_block(&text),
            false => text.clone(),
        };

        out.push(RefFile {
            lines: text.lines().count(),
            functions: top_level_functions(&source),
            path,
        });
    }
}

fn arrow_binding(line: &str) -> Option<&str> {
    let rest = line
        .strip_prefix("export const ")
        .or_else(|| line.strip_prefix("const "))?;

    let (name, body) = rest.split_once(" = ")?;

    if name.is_empty() || name.contains(|c: char| !c.is_alphanumeric() && c != '_') {
        return None;
    }

    let body = body.strip_prefix("async ").unwrap_or(body);

    let opens_with_arguments = body.starts_with('(')
        || body
            .split_once("=>")
            .is_some_and(|(before, _)| !before.contains(['(', '.', '[']));

    match opens_with_arguments {
        true => Some(rest),
        false => None,
    }
}

fn top_level_functions(text: &str) -> Vec<String> {
    let mut out = Vec::new();

    for line in text.lines() {
        let rest = line
            .strip_prefix("export function ")
            .or_else(|| line.strip_prefix("function "))
            .or_else(|| line.strip_prefix("export function* "))
            .or_else(|| line.strip_prefix("function* "))
            .or_else(|| arrow_binding(line));

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

fn collect_our_source(root: &Path) -> String {
    let mut out = String::new();

    for crate_dir in ["poe-wayfinder-core/src", "poe-wayfinder-app/src"] {
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

    let parity = if total == 0 {
        100.0
    } else {
        ((ported + waived) as f64 / total as f64) * 100.0
    };

    println!("poe-wayfinder parity report");
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
        let ours = "pub fn parse_foo_bar() {}";

        assert_eq!(status_of("parseFoo", ours), Status::Missing);
    }

    #[test]
    fn an_aliased_function_is_recognised() {
        let ours = "pub fn text_to_sections(text: &str) {}";

        assert_eq!(status_of("itemTextToSections", ours), Status::Ported);
    }

    #[test]
    fn an_alias_that_points_nowhere_is_still_missing() {
        assert_eq!(status_of("itemTextToSections", ""), Status::Missing);
    }

    #[test]
    fn no_function_is_both_aliased_and_waived() {
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
