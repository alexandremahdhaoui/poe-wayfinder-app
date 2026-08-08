use std::path::PathBuf;
use std::process::ExitCode;

use poe_trader_app::adapter::game_data_adapter::GameTables;
use poe_trader_core::adapter::data_adapter::Namespace;
use poe_trader_core::adapter::StatLookup;
use poe_trader_core::controller::parse::parse_clipboard;
use poe_trader_core::controller::stat_match::placeholder::candidates;
use poe_trader_core::types::category::ItemCategory;
use poe_trader_core::types::item::BaseInfo;
use poe_trader_core::types::GameVersion;

const ROLL: &str = "7";

fn main() -> ExitCode {
    let mut dirs: Vec<PathBuf> = Vec::new();
    let mut min = 0.0_f64;
    let mut show = 20_usize;
    let mut args = std::env::args().skip(1);

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--min" => min = args.next().and_then(|v| v.parse().ok()).unwrap_or(0.0),
            "--show" => show = args.next().and_then(|v| v.parse().ok()).unwrap_or(20),
            other => dirs.push(PathBuf::from(other)),
        }
    }

    if dirs.is_empty() {
        eprintln!("poe-trader-datacheck: give at least one data directory");

        return ExitCode::FAILURE;
    }

    let mut worst = 100.0_f64;

    for dir in &dirs {
        let tables = match GameTables::load(dir) {
            Ok(tables) => tables,
            Err(err) => {
                eprintln!("poe-trader-datacheck: loading {}: {err}", dir.display());

                return ExitCode::FAILURE;
            }
        };

        let game = game_of(dir);
        let report = check(&tables);

        println!("\n{} as {}", dir.display(), game.as_str());
        println!("  stat templates : {}", report.total);
        println!("  read back      : {}", report.hit);
        println!("  missed         : {}", report.missed.len());
        println!("  coverage       : {:.1}%", report.coverage());

        for (reference, template, rendered) in report.missed.iter().take(show) {
            println!("    {reference}\n      via {template}\n      as  {rendered}");
        }

        if report.missed.len() > show {
            println!("    ... {} more", report.missed.len() - show);
        }

        let bases = check_items(&tables, game);

        println!("  bases          : {}", bases.total);
        println!("  named back     : {}", bases.hit);
        println!("  missed         : {}", bases.missed.len());
        println!("  coverage       : {:.1}%", bases.coverage());

        for (name, _, got) in bases.missed.iter().take(show) {
            println!("    {name}\n      came back as {got}");
        }

        if bases.missed.len() > show {
            println!("    ... {} more", bases.missed.len() - show);
        }

        let empty = empty_tables(&tables, game);

        if empty.is_empty() {
            println!("  tables         : every table the parser reads has entries");
        } else {
            println!("  tables         : EMPTY {}", empty.join(", "));

            worst = 0.0;
        }

        worst = worst.min(report.coverage()).min(bases.coverage());
    }

    if worst < min {
        eprintln!("\npoe-trader-datacheck: coverage {worst:.1}% is below the floor of {min:.1}%");

        return ExitCode::FAILURE;
    }

    ExitCode::SUCCESS
}

struct Report {
    total: usize,
    hit: usize,
    missed: Vec<(String, String, String)>,
}

impl Report {
    fn coverage(&self) -> f64 {
        if self.total == 0 {
            return 0.0;
        }

        self.hit as f64 * 100.0 / self.total as f64
    }
}

fn check(tables: &GameTables) -> Report {
    let mut report = Report {
        total: 0,
        hit: 0,
        missed: Vec::new(),
    };

    for (reference, template) in tables.matchers() {
        let rendered = render(template);

        report.total += 1;

        let found = candidates(&rendered)
            .iter()
            .filter_map(|c| tables.stat_by_matcher(&c.template))
            .any(|hit| hit.stat.reference == reference);

        if found {
            report.hit += 1;
        } else {
            report
                .missed
                .push((reference.to_string(), template.to_string(), rendered));
        }
    }

    report
}

fn game_of(dir: &std::path::Path) -> GameVersion {
    if dir.to_string_lossy().contains("poe1") {
        GameVersion::Poe1
    } else {
        GameVersion::Poe2
    }
}

fn empty_tables(tables: &GameTables, game: GameVersion) -> Vec<&'static str> {
    let mut wanted = vec![Namespace::Item, Namespace::Unique, Namespace::Gem];

    if game == GameVersion::Poe1 {
        wanted.push(Namespace::DivinationCard);
        wanted.push(Namespace::CapturedBeast);
    }

    wanted
        .into_iter()
        .filter(|want| !tables.items().any(|(ns, _)| ns == *want))
        .map(|ns| ns.as_str())
        .collect()
}

fn check_items(tables: &GameTables, game: GameVersion) -> Report {
    let mut report = Report {
        total: 0,
        hit: 0,
        missed: Vec::new(),
    };

    for (namespace, base) in tables.items() {
        let Some(clipboard) = clipboard_for(namespace, base) else {
            continue;
        };

        report.total += 1;

        let got = parse_clipboard(&clipboard, game, tables)
            .map(|item| item.info.name)
            .unwrap_or_default();

        if got == base.name {
            report.hit += 1;
        } else {
            report
                .missed
                .push((base.name.clone(), clipboard, format!("{got:?}")));
        }
    }

    report
}

fn clipboard_for(namespace: Namespace, base: &BaseInfo) -> Option<String> {
    let name = &base.name;

    match namespace {
        Namespace::Item if base.category == Some(ItemCategory::Currency) => Some(format!(
            "Item Class: Stackable Currency\nRarity: Currency\n{name}\n--------\nStack Size: 1/10\n"
        )),
        Namespace::Item => Some(format!(
            "Item Class: Unknown\nRarity: Normal\n{name}\n--------\nItem Level: 80\n"
        )),
        Namespace::Gem => Some(format!(
            "Item Class: Skill Gems\nRarity: Gem\n{name}\n--------\nLevel: 20\n"
        )),
        Namespace::Unique | Namespace::DivinationCard | Namespace::CapturedBeast => None,
    }
}

fn render(template: &str) -> String {
    template.replace('#', ROLL)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_placeholder_becomes_a_roll() {
        assert_eq!(render("# to maximum Life"), "7 to maximum Life");
    }

    #[test]
    fn the_sign_in_a_template_survives_rendering() {
        assert_eq!(render("+# to maximum Life"), "+7 to maximum Life");
    }

    #[test]
    fn a_template_with_two_placeholders_fills_both() {
        assert_eq!(render("Adds # to # Fire Damage"), "Adds 7 to 7 Fire Damage");
    }

    #[test]
    fn a_template_with_no_placeholder_is_itself() {
        assert_eq!(render("Has Alt Ailment"), "Has Alt Ailment");
    }

    #[test]
    fn coverage_of_nothing_is_zero_and_not_a_division_by_zero() {
        let report = Report {
            total: 0,
            hit: 0,
            missed: Vec::new(),
        };

        assert_eq!(report.coverage(), 0.0);
    }

    #[test]
    fn coverage_counts_hits_against_the_total() {
        let report = Report {
            total: 4,
            hit: 3,
            missed: Vec::new(),
        };

        assert_eq!(report.coverage(), 75.0);
    }

    #[test]
    fn the_roll_is_not_a_value_matchers_bake_in() {
        assert_ne!(ROLL, "0");
        assert_ne!(ROLL, "1");
    }
}
