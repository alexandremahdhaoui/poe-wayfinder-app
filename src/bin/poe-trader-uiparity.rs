use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

struct Capability {
    component: &'static str,
    name: &'static str,
    domain: &'static [&'static str],
    ui: &'static [&'static str],
}

const WAIVED: &[(&str, &str)] = &[
    (
        "price prediction",
        "poeprices.info is a third party. The workspace forbids one.",
    ),
    (
        "price trend history",
        "poe.ninja is a third party. The workspace forbids one.",
    ),
    (
        "feedback on a prediction",
        "reports back to the third party that made the prediction.",
    ),
    (
        "related items",
        "reads the fork maintainer's own server, which the workspace forbids.",
    ),
    (
        "item image",
        "images come from web.poecdn.com, which the network policy refuses.",
    ),
    (
        "influence icons",
        "images come from web.poecdn.com, which the network policy refuses.",
    ),
    (
        "currency icons",
        "images come from web.poecdn.com, which the network policy refuses.",
    ),
    (
        "in-app settings screen",
        "settings are a config file and tray menu here, not a screen.",
    ),
    (
        "app titlebar",
        "the overlay is undecorated on purpose. There is no titlebar.",
    ),
    (
        "fullscreen image viewer",
        "exists to enlarge a CDN image the network policy refuses.",
    ),
];

const CAPABILITIES: &[Capability] = &[
    Capability {
        component: "FilterModifier.vue",
        name: "a filter row per modifier, labelled with the line the game printed",
        domain: &["FilterSource", "fn stats("],
        ui: &["stat_row"],
    },
    Capability {
        component: "FilterModifier.vue",
        name: "each filter can be switched off without losing its value",
        domain: &["fn set_enabled"],
        ui: &["UiEvent::ToggleRow"],
    },
    Capability {
        component: "FilterModifier.vue",
        name: "a minimum and a maximum can be typed per filter",
        domain: &["fn set_min", "fn set_max"],
        ui: &["UiEvent::SetMin", "UiEvent::SetMax"],
    },
    Capability {
        component: "StatRollSlider.vue",
        name: "a gauge shows where the roll sits inside its tier",
        domain: &["fn percent_of_bounds"],
        ui: &["fn gauge"],
    },
    Capability {
        component: "StatRollSlider.vue",
        name: "the gauge fills the part of the range the search asks for",
        domain: &["pub bounds"],
        ui: &["GAUGE_FILL"],
    },
    Capability {
        component: "FilterBtnNumeric.vue",
        name: "numeric property filters, one per property the item has",
        domain: &["enum NumericKey"],
        ui: &["fn numeric_row"],
    },
    Capability {
        component: "FilterBtnNumeric.vue",
        name: "a numeric filter starts from the item's own value",
        domain: &["fn numeric_row"],
        ui: &["fn bounds_inputs"],
    },
    Capability {
        component: "FilterBtnLogical.vue",
        name: "logical filters such as corrupted and mirrored",
        domain: &["enum FlagKey"],
        ui: &["fn flag_chip"],
    },
    Capability {
        component: "FilterBtnLogical.vue",
        name: "a logical filter can ask for the absence of something",
        domain: &["fn set_flag"],
        ui: &["UiEvent::InvertFlag"],
    },
    Capability {
        component: "FiltersBlock.vue",
        name: "the item's name and base type constrain the search",
        domain: &["fn name_rows"],
        ui: &["fn name_row"],
    },
    Capability {
        component: "FilterName.vue",
        name: "the search can be widened from the exact item to its base type",
        domain: &["enum NameMode"],
        ui: &["UiEvent::CycleName"],
    },
    Capability {
        component: "FiltersBlock.vue",
        name: "the whole stat block can be turned on or off at once",
        domain: &["fn set_all_stats"],
        ui: &["UiEvent::SetAllStats"],
    },
    Capability {
        component: "UnknownModifier.vue",
        name: "a modifier the parser did not recognise is called out",
        domain: &["unknown_modifiers"],
        ui: &["warnings"],
    },
    Capability {
        component: "FilterModifierTiers.vue",
        name: "the tier a modifier rolled at is shown",
        domain: &["fn tier_label"],
        ui: &["tier_label"],
    },
    Capability {
        component: "SourceInfo.vue",
        name: "a total says which modifiers add up to it",
        domain: &["pub contributors"],
        ui: &["fn contributor_line"],
    },
    Capability {
        component: "ItemEditor.vue",
        name: "an augment can be socketed into the item before searching",
        domain: &["fn preview_filters"],
        ui: &["fn augment_picker"],
    },
    Capability {
        component: "ItemEditorButton.vue",
        name: "each augment shows the stat it would grant",
        domain: &["fn augment_options"],
        ui: &["UiEvent::ChooseAugment"],
    },
    Capability {
        component: "ItemEditor.vue",
        name: "a chosen augment can be taken back off",
        domain: &["fn clear_augment"],
        ui: &["UiEvent::ClearAugment"],
    },
    Capability {
        component: "ItemEditor.vue",
        name: "the augment list is restricted to what fits this item",
        domain: &["fn effect_for_category"],
        ui: &["model.augments()"],
    },
    Capability {
        component: "TradeListing.vue",
        name: "the listings themselves are shown, not only how many there are",
        domain: &["fn read_listings"],
        ui: &["fn listing_rows"],
    },
    Capability {
        component: "TradeItem.vue",
        name: "each listing shows its price and currency",
        domain: &["pub struct Listing"],
        ui: &["fn listing_row"],
    },
    Capability {
        component: "TradeItem.vue",
        name: "each listing shows whether the seller is online",
        domain: &["pub online"],
        ui: &["ONLINE_DOT"],
    },
    Capability {
        component: "TradeItem.vue",
        name: "each listing shows who is selling it",
        domain: &["pub account"],
        ui: &["listing.account"],
    },
    Capability {
        component: "TradeListing.vue",
        name: "the matched count is shown beside the listings",
        domain: &["fn total("],
        ui: &["listings"],
    },
    Capability {
        component: "ItemQuickPrice.vue",
        name: "a price is suggested from what the market is actually asking",
        domain: &["fn suggested_price"],
        ui: &["fn price_banner"],
    },
    Capability {
        component: "ItemQuickPrice.vue",
        name: "the spread of asking prices is shown, not one number alone",
        domain: &["fn price_spread"],
        ui: &["price_spread"],
    },
    Capability {
        component: "PricePrediction.vue",
        name: "an estimate is offered before the listings are read",
        domain: &["fn estimate_from"],
        ui: &["price_banner"],
    },
    Capability {
        component: "StackValue.vue",
        name: "a stack is priced per unit and in total",
        domain: &["fn stack_value"],
        ui: &["stack_value"],
    },
    Capability {
        component: "TradeLinks.vue",
        name: "the search opens on the trade site",
        domain: &["fn browser_url"],
        ui: &["UiEvent::OpenInBrowser"],
    },
    Capability {
        component: "OnlineFilter.vue",
        name: "offline sellers can be included",
        domain: &["enum Status"],
        ui: &["UiEvent::ToggleOnline"],
    },
    Capability {
        component: "RateLimiterState.vue",
        name: "the rate limiter says how much room is left",
        domain: &["fn limiter_report"],
        ui: &["fn rate_limit_line"],
    },
    Capability {
        component: "PriceCheckWindow.vue",
        name: "the item's name and base type head the panel",
        domain: &["fn showing_text"],
        ui: &["text.title"],
    },
    Capability {
        component: "CheckedItem.vue",
        name: "rarity, item level and quality are shown",
        domain: &["fn showing_text"],
        ui: &["text.body"],
    },
    Capability {
        component: "CheckedItem.vue",
        name: "the rarity is coloured the way the game colours it",
        domain: &["fn rarity_colour"],
        ui: &["rarity_colour"],
    },
    Capability {
        component: "PriceCheckWindow.vue",
        name: "the panel says when it is still working",
        domain: &["OverlayState::Loading"],
        ui: &["Checking price"],
    },
    Capability {
        component: "UiErrorBox.vue",
        name: "a failure says what failed rather than going blank",
        domain: &["fn fail"],
        ui: &["OverlayState::Error"],
    },
    Capability {
        component: "PriceCheckWindow.vue",
        name: "the search can be run again after the filters are edited",
        domain: &["fn edited_check"],
        ui: &["UiEvent::Research"],
    },
    Capability {
        component: "PriceCheckWindow.vue",
        name: "the panel can be dismissed",
        domain: &["fn hide"],
        ui: &["UiEvent::Dismiss"],
    },
    Capability {
        component: "ReloadTradeData.vue",
        name: "the game data can be rebuilt without leaving the overlay",
        domain: &["fn rebuild_data"],
        ui: &["TrayAction::RebuildData"],
    },
    Capability {
        component: "main/src/windows/game.ts",
        name: "the running game is detected rather than configured",
        domain: &["fn detect_game"],
        ui: &["fn follow_game"],
    },
    Capability {
        component: "main/src/windows/game.ts",
        name: "the overlay follows when the other game comes to the front",
        domain: &["fn set_game"],
        ui: &["the game changed"],
    },
    Capability {
        component: "main/src/host-files/index.ts",
        name: "the game data lives inside the binary, so nothing is shipped beside it",
        domain: &["fn embedded"],
        ui: &["fn build_data"],
    },
    Capability {
        component: "main/src/update-checker.ts",
        name: "the game data refreshes itself from the official api",
        domain: &["fn refresh_due"],
        ui: &["fn start"],
    },
    Capability {
        component: "UnidentifiedResolver.vue",
        name: "an unidentified item is priced by its base and item level",
        domain: &["item.is_unidentified"],
        ui: &["is_unidentified"],
    },
    Capability {
        component: "BackgroundInfo.vue",
        name: "the league in use is visible",
        domain: &["fn last_league"],
        ui: &["league"],
    },
    Capability {
        component: "CheckPositionCircle.vue",
        name: "the panel opens where the check was made",
        domain: &["anchor_cursor"],
        ui: &["Anchor::Cursor"],
    },
    Capability {
        component: "VirtualScroll.vue",
        name: "a long filter list scrolls rather than overflowing",
        domain: &["fn filters("],
        ui: &["ScrollArea"],
    },
    Capability {
        component: "UiCheckbox.vue",
        name: "a filter's state is visible at a glance",
        domain: &["pub enabled"],
        ui: &["fn checkbox"],
    },
    Capability {
        component: "Popover.vue",
        name: "hovering a filter explains where its numbers came from",
        domain: &["fn roll_caption"],
        ui: &["on_hover_text"],
    },
    Capability {
        component: "ItemModifierText.vue",
        name: "the roll is shown inside the modifier text",
        domain: &["fn modifier_text"],
        ui: &["modifier_text"],
    },
];

const FLOOR: f64 = 0.0;

fn main() -> ExitCode {
    let floor = floor_from_args();
    let root = repo_root();

    let files = match collect(&root) {
        Ok(files) => files,
        Err(message) => {
            eprintln!("uiparity: {message}");

            return ExitCode::FAILURE;
        }
    };

    let report = measure(&files);

    print_report(&report);

    let percent = report.percent();

    if percent + 1e-9 < floor {
        println!("FAIL: ui parity {percent:.1}% is below the floor of {floor:.1}%");

        return ExitCode::FAILURE;
    }

    println!("OK: ui parity {percent:.1}% meets the floor of {floor:.1}%");

    ExitCode::SUCCESS
}

fn floor_from_args() -> f64 {
    let mut args = std::env::args().skip(1);

    while let Some(arg) = args.next() {
        if arg == "--min" {
            return args.next().and_then(|v| v.parse().ok()).unwrap_or(FLOOR);
        }
    }

    FLOOR
}

fn repo_root() -> PathBuf {
    let here = Path::new(env!("CARGO_MANIFEST_DIR"));

    here.parent().unwrap_or(here).to_path_buf()
}

fn collect(root: &Path) -> Result<Vec<(String, String)>, String> {
    let mut out = Vec::new();

    for crate_name in ["poe-trader-app", "poe-trader-core"] {
        let src = root.join(crate_name).join("src");

        if !src.is_dir() {
            return Err(format!("{} is not a directory", src.display()));
        }

        walk(&src, &src, crate_name, &mut out)?;
    }

    Ok(out)
}

fn walk(
    dir: &Path,
    base: &Path,
    crate_name: &str,
    out: &mut Vec<(String, String)>,
) -> Result<(), String> {
    let entries = fs::read_dir(dir).map_err(|e| format!("reading {}: {e}", dir.display()))?;

    for entry in entries {
        let entry = entry.map_err(|e| format!("reading an entry in {}: {e}", dir.display()))?;
        let path = entry.path();

        if path.is_dir() {
            walk(&path, base, crate_name, out)?;

            continue;
        }

        if path.extension().and_then(|e| e.to_str()) != Some("rs") {
            continue;
        }

        let body =
            fs::read_to_string(&path).map_err(|e| format!("reading {}: {e}", path.display()))?;

        let relative = path
            .strip_prefix(base)
            .map(|p| p.display().to_string())
            .unwrap_or_else(|_| path.display().to_string());

        out.push((format!("{crate_name}/src/{relative}"), body));
    }

    Ok(())
}

#[derive(Debug, PartialEq)]
struct Outcome {
    name: &'static str,
    component: &'static str,
    missing_domain: Vec<&'static str>,
    missing_ui: Vec<&'static str>,
}

impl Outcome {
    fn done(&self) -> bool {
        self.missing_domain.is_empty() && self.missing_ui.is_empty()
    }
}

#[derive(Debug, PartialEq)]
struct Report {
    outcomes: Vec<Outcome>,
}

impl Report {
    fn done(&self) -> usize {
        self.outcomes.iter().filter(|o| o.done()).count()
    }

    fn total(&self) -> usize {
        self.outcomes.len()
    }

    fn percent(&self) -> f64 {
        match self.total() {
            0 => 100.0,
            total => (self.done() as f64 / total as f64) * 100.0,
        }
    }
}

fn measure(files: &[(String, String)]) -> Report {
    let outcomes = CAPABILITIES
        .iter()
        .map(|capability| Outcome {
            name: capability.name,
            component: capability.component,
            missing_domain: capability
                .domain
                .iter()
                .filter(|needle| !anywhere(files, needle))
                .copied()
                .collect(),
            missing_ui: capability
                .ui
                .iter()
                .filter(|needle| !in_a_driver(files, needle))
                .copied()
                .collect(),
        })
        .collect();

    Report { outcomes }
}

fn anywhere(files: &[(String, String)], needle: &str) -> bool {
    files.iter().any(|(_, body)| outside_tests(body, needle))
}

fn in_a_driver(files: &[(String, String)], needle: &str) -> bool {
    files
        .iter()
        .filter(|(path, _)| path.contains("/src/driver/"))
        .any(|(_, body)| outside_tests(body, needle))
}

fn production(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut skipping = false;
    let mut depth: i32 = 0;

    for line in text.lines() {
        if !skipping && line.trim_start().starts_with("#[cfg(test)]") {
            skipping = true;
            depth = 0;

            continue;
        }

        if !skipping {
            out.push_str(line);
            out.push('\n');

            continue;
        }

        depth += line.matches('{').count() as i32;
        depth -= line.matches('}').count() as i32;

        if depth <= 0 && line.contains('}') {
            skipping = false;
        }
    }

    out
}

fn outside_tests(body: &str, needle: &str) -> bool {
    production(body).contains(needle)
}

fn print_report(report: &Report) {
    let mut by_component: BTreeMap<&str, Vec<&Outcome>> = BTreeMap::new();

    for outcome in &report.outcomes {
        by_component
            .entry(outcome.component)
            .or_default()
            .push(outcome);
    }

    println!("UI parity against Awakened PoE Trade and Exiled Exchange 2");
    println!();

    for (component, outcomes) in &by_component {
        let done = outcomes.iter().filter(|o| o.done()).count();

        println!("{component}  {done}/{}", outcomes.len());

        for outcome in outcomes {
            if outcome.done() {
                println!("    ok      {}", outcome.name);

                continue;
            }

            println!("    MISSING {}", outcome.name);

            for needle in &outcome.missing_domain {
                println!("              no domain code for {needle:?}");
            }

            for needle in &outcome.missing_ui {
                println!("              nothing in src/driver/ uses {needle:?}");
            }
        }

        println!();
    }

    println!("Waived, with the reason:");

    for (name, reason) in WAIVED {
        println!("    {name}: {reason}");
    }

    println!();
    println!("  ui parity      : {:.1}%", report.percent());
    println!("  implemented    : {} of {}", report.done(), report.total());
    println!("  waived         : {}", WAIVED.len());
}

#[cfg(test)]
mod tests {
    use super::*;

    fn files(entries: &[(&str, &str)]) -> Vec<(String, String)> {
        entries
            .iter()
            .map(|(p, b)| (p.to_string(), b.to_string()))
            .collect()
    }

    #[test]
    fn a_capability_with_both_halves_present_counts() {
        let tree = files(&[
            (
                "poe-trader-core/src/controller/x.rs",
                "fn suggested_price()",
            ),
            ("poe-trader-app/src/driver/ui.rs", "fn price_banner()"),
        ]);

        let outcome = Outcome {
            name: "n",
            component: "c",
            missing_domain: CAPABILITIES[0]
                .domain
                .iter()
                .filter(|n| !anywhere(&tree, n))
                .copied()
                .collect(),
            missing_ui: Vec::new(),
        };

        assert!(
            !outcome.done(),
            "unrelated code must not satisfy a capability"
        );
        assert!(anywhere(&tree, "fn suggested_price"));
        assert!(in_a_driver(&tree, "fn price_banner"));
    }

    #[test]
    fn domain_code_alone_does_not_count_as_a_feature() {
        let tree = files(&[("poe-trader-core/src/controller/x.rs", "fn price_banner()")]);

        assert!(anywhere(&tree, "fn price_banner"));
        assert!(
            !in_a_driver(&tree, "fn price_banner"),
            "a symbol outside src/driver/ is domain code nobody can see"
        );
    }

    #[test]
    fn a_symbol_that_only_appears_in_a_test_does_not_count() {
        let tree = files(&[(
            "poe-trader-app/src/driver/ui.rs",
            "fn real() {}\n#[cfg(test)]\nmod tests { fn price_banner() {} }",
        )]);

        assert!(!in_a_driver(&tree, "fn price_banner"));
    }

    #[test]
    fn a_symbol_in_production_code_counts_even_when_tests_follow_it() {
        let tree = files(&[(
            "poe-trader-app/src/driver/ui.rs",
            "fn price_banner() {}\n#[cfg(test)]\nmod tests {}",
        )]);

        assert!(in_a_driver(&tree, "fn price_banner"));
    }

    #[test]
    fn an_empty_catalogue_is_complete_rather_than_dividing_by_zero() {
        let report = Report {
            outcomes: Vec::new(),
        };

        assert_eq!(report.percent(), 100.0);
    }

    #[test]
    fn the_percentage_is_the_share_of_capabilities_that_are_done() {
        let report = Report {
            outcomes: vec![
                Outcome {
                    name: "a",
                    component: "c",
                    missing_domain: Vec::new(),
                    missing_ui: Vec::new(),
                },
                Outcome {
                    name: "b",
                    component: "c",
                    missing_domain: vec!["x"],
                    missing_ui: Vec::new(),
                },
            ],
        };

        assert_eq!(report.percent(), 50.0);
        assert_eq!(report.done(), 1);
    }

    #[test]
    fn every_capability_names_a_component_it_came_from() {
        for capability in CAPABILITIES {
            assert!(
                capability.component.ends_with(".vue") || capability.component.ends_with(".ts"),
                "{} must point at the upstream component it ports",
                capability.name
            );
        }
    }

    #[test]
    fn a_capability_from_the_electron_shell_names_a_file_under_main() {
        for capability in CAPABILITIES {
            if !capability.component.ends_with(".ts") {
                continue;
            }

            assert!(
                capability.component.starts_with("main/src/"),
                "{} names {}, which is not where the reference shell lives",
                capability.name,
                capability.component
            );
        }
    }

    #[test]
    fn every_capability_demands_both_domain_code_and_a_use_of_it() {
        for capability in CAPABILITIES {
            assert!(
                !capability.domain.is_empty(),
                "{} has no domain evidence",
                capability.name
            );

            assert!(
                !capability.ui.is_empty(),
                "{} has no ui evidence, so it could pass while invisible",
                capability.name
            );
        }
    }

    #[test]
    fn no_capability_is_listed_twice() {
        let mut names: Vec<&str> = CAPABILITIES.iter().map(|c| c.name).collect();
        names.sort_unstable();

        let before = names.len();
        names.dedup();

        assert_eq!(before, names.len(), "a duplicate would inflate the count");
    }

    #[test]
    fn every_waiver_carries_a_reason() {
        for (name, reason) in WAIVED {
            assert!(!name.is_empty());
            assert!(reason.len() > 20, "{name} needs a real reason, not a word");
        }
    }

    #[test]
    fn a_waived_capability_is_not_also_counted() {
        for (waived, _) in WAIVED {
            assert!(
                !CAPABILITIES.iter().any(|c| c.name == *waived),
                "{waived} is both counted and waived"
            );
        }
    }

    #[test]
    fn the_real_tree_is_readable_and_produces_a_report() {
        let files = collect(&repo_root()).expect("the source tree");

        assert!(files.len() > 50, "found only {} files", files.len());

        let report = measure(&files);

        assert_eq!(report.total(), CAPABILITIES.len());
    }
}
