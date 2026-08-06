//! How much of a data file the parser can actually read.
//!
//! # Why this exists
//!
//! The overlay looked finished and could not match 1072 PoE1 stats, because
//! the trade data keys some stats with a leading `+` and the parser dropped
//! it. Nothing failed. The filters were simply absent and the item priced
//! against the whole market.
//!
//! A silent miss cannot be found by reading code. It can be counted.
//!
//! For every matcher template in a data file this renders the line the game
//! would print, feeds it to the same matcher the parser uses, and checks the
//! answer is the stat it came from. A template that does not come back is
//! printed with its reference.
//!
//! ```sh
//! cargo run --bin poe-trader-datacheck -- data-poe1
//! ```
//!
//! Exit code is 1 when coverage is below the floor given by `--min`, so this
//! runs as a forge stage rather than as something to remember to look at.

use std::path::PathBuf;
use std::process::ExitCode;

use poe_trader_app::adapter::game_data_adapter::GameTables;
use poe_trader_core::adapter::StatLookup;
use poe_trader_core::controller::stat_match::placeholder::candidates;

/// The roll written into a rendered line.
///
/// Any value works. Seven is a whole number, so no template is accidentally
/// tested against the decimal path, and it is not 0 or 1, which some matchers
/// bake in as a literal.
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

        let report = check(&tables);

        println!("\n{}", dir.display());
        println!("  templates  : {}", report.total);
        println!("  read back  : {}", report.hit);
        println!("  missed     : {}", report.missed.len());
        println!("  coverage   : {:.1}%", report.coverage());

        for (reference, template, rendered) in report.missed.iter().take(show) {
            println!("    {reference}\n      via {template}\n      as  {rendered}");
        }

        if report.missed.len() > show {
            println!("    ... {} more", report.missed.len() - show);
        }

        worst = worst.min(report.coverage());
    }

    if worst < min {
        eprintln!("\npoe-trader-datacheck: coverage {worst:.1}% is below the floor of {min:.1}%");

        return ExitCode::FAILURE;
    }

    ExitCode::SUCCESS
}

/// What a run found.
struct Report {
    total: usize,
    hit: usize,
    /// Reference, the template that was not read back, and the line rendered
    /// from it.
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

/// Render every template and check it reads back as its own stat.
fn check(tables: &GameTables) -> Report {
    let mut report = Report {
        total: 0,
        hit: 0,
        missed: Vec::new(),
    };

    for (reference, template) in tables.matchers() {
        let rendered = render(template);

        report.total += 1;

        // The reference is the identity being checked, not the template. Two
        // matchers on one stat are two spellings of the same thing, and either
        // one coming back is the right answer.
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

/// Turn a matcher template into the line the game would print.
///
/// The only substitution is the placeholder. Everything else in a template is
/// literal text the game prints unchanged.
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
        // Rendering "+# to maximum Life" as "7 to maximum Life" would drop the
        // very thing this tool was written to catch and report full coverage.
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
        // A matcher can carry `"value": 1`, meaning the text has no number and
        // the stat is worth one. Rendering with 1 would make those look like
        // they matched a placeholder.
        assert_ne!(ROLL, "0");
        assert_ne!(ROLL, "1");
    }
}
