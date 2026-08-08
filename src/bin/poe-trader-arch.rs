use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

const MAIN_BINARY: &str = "src/bin/poe-trader.rs";
const MAIN_MAX_LINES: usize = 220;

const WAIVED: &[(&str, &str)] = &[(
    "src/driver/cli_driver.rs",
    "diagnostics and self tests read adapters directly on purpose, because \
     their whole job is to report what an adapter sees before any controller exists",
)];

#[derive(Debug, Clone, PartialEq, Eq)]
struct Violation {
    rule: &'static str,
    path: String,
    detail: String,
}

struct Source {
    path: PathBuf,
    relative: String,
    text: String,
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();

    let root = args
        .iter()
        .position(|a| a == "--root")
        .and_then(|i| args.get(i + 1))
        .map_or_else(|| PathBuf::from("."), PathBuf::from);

    let ceiling: usize = args
        .iter()
        .position(|a| a == "--max")
        .and_then(|i| args.get(i + 1))
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);

    let src = root.join("src");

    if !src.is_dir() {
        eprintln!("poe-trader-arch: no src directory at {}", src.display());

        return ExitCode::FAILURE;
    }

    let mut sources = Vec::new();
    collect(&src, &root, &mut sources);

    report(&sources, ceiling)
}

fn collect(dir: &Path, root: &Path, out: &mut Vec<Source>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };

    for entry in entries.flatten() {
        let path = entry.path();

        if path.is_dir() {
            collect(&path, root, out);

            continue;
        }

        if path.extension().and_then(|e| e.to_str()) != Some("rs") {
            continue;
        }

        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };

        let relative = path
            .strip_prefix(root)
            .unwrap_or(&path)
            .to_string_lossy()
            .replace('\\', "/");

        out.push(Source {
            path: path.clone(),
            relative,
            text,
        });
    }
}

fn is_generated(relative: &str) -> bool {
    relative.contains("zz_generated")
}

fn is_waived(relative: &str) -> bool {
    WAIVED.iter().any(|(path, _)| *path == relative)
}

fn check(sources: &[Source]) -> Vec<Violation> {
    let mut found = Vec::new();

    for source in sources {
        if is_generated(&source.relative) {
            continue;
        }

        found.extend(main_is_only_wiring(source));
        found.extend(no_comments(source));

        if is_waived(&source.relative) {
            continue;
        }

        found.extend(layer_imports(source));
        found.extend(hand_written_fakes(source));
    }

    found.extend(modules_are_declared(sources));

    found
}

fn main_is_only_wiring(source: &Source) -> Vec<Violation> {
    if source.relative != MAIN_BINARY {
        return Vec::new();
    }

    let lines = source.text.lines().count();

    if lines <= MAIN_MAX_LINES {
        return Vec::new();
    }

    vec![Violation {
        rule: "main is only wiring",
        path: source.relative.clone(),
        detail: format!("{lines} lines, at most {MAIN_MAX_LINES} allowed"),
    }]
}

fn layer_imports(source: &Source) -> Vec<Violation> {
    let (layer, forbidden, why) = if source.relative.starts_with("src/driver/") {
        ("driver", "crate::adapter::", "a driver reaches an adapter")
    } else if source.relative.starts_with("src/controller/") {
        (
            "controller",
            "crate::driver::",
            "a controller reaches a driver",
        )
    } else {
        return Vec::new();
    };

    let _ = layer;

    source
        .text
        .lines()
        .enumerate()
        .filter(|(_, line)| line.trim_start().starts_with("use ") && line.contains(forbidden))
        .map(|(n, line)| Violation {
            rule: "layers only depend downward",
            path: source.relative.clone(),
            detail: format!("line {}: {why}: {}", n + 1, line.trim()),
        })
        .collect()
}

fn hand_written_fakes(source: &Source) -> Vec<Violation> {
    source
        .text
        .lines()
        .enumerate()
        .filter(|(_, line)| {
            let t = line.trim_start();

            ["struct Fake", "struct Stub", "struct Mock"]
                .iter()
                .any(|p| starts_a_fake(t, p))
        })
        .map(|(n, line)| Violation {
            rule: "test doubles are generated or named for what they do",
            path: source.relative.clone(),
            detail: format!("line {}: {}", n + 1, line.trim()),
        })
        .collect()
}

fn starts_a_fake(line: &str, prefix: &str) -> bool {
    let Some(rest) = line.strip_prefix(prefix) else {
        return false;
    };

    !rest.starts_with(|c: char| c.is_lowercase())
}

fn no_comments(source: &Source) -> Vec<Violation> {
    source
        .text
        .lines()
        .enumerate()
        .filter(|(_, line)| {
            let t = line.trim_start();

            t.starts_with("//") || t.starts_with("/*")
        })
        .map(|(n, line)| Violation {
            rule: "no comments",
            path: source.relative.clone(),
            detail: format!("line {}: {}", n + 1, line.trim()),
        })
        .collect()
}

fn modules_are_declared(sources: &[Source]) -> Vec<Violation> {
    let declarations: String = sources
        .iter()
        .filter(|s| s.path.file_name().and_then(|f| f.to_str()) == Some("mod.rs"))
        .map(|s| s.text.clone())
        .collect::<Vec<_>>()
        .join("\n");

    let lib = sources
        .iter()
        .find(|s| s.relative == "src/lib.rs")
        .map(|s| s.text.clone())
        .unwrap_or_default();

    let all = format!("{declarations}\n{lib}");

    sources
        .iter()
        .filter(|s| {
            let name = s.path.file_name().and_then(|f| f.to_str()).unwrap_or("");

            !matches!(name, "mod.rs" | "lib.rs")
                && !s.relative.starts_with("src/bin/")
                && !is_generated(&s.relative)
        })
        .filter(|s| {
            let stem = s.path.file_stem().and_then(|f| f.to_str()).unwrap_or("");

            !all.contains(&format!("mod {stem};"))
        })
        .map(|s| Violation {
            rule: "every module is declared",
            path: s.relative.clone(),
            detail: "no mod declaration, so this file never compiles".to_string(),
        })
        .collect()
}

fn report(sources: &[Source], ceiling: usize) -> ExitCode {
    let found = check(sources);

    println!("poe-trader architecture report\n");
    println!("  files scanned  : {}", sources.len());
    println!("  violations     : {}", found.len());
    println!("  ceiling        : {ceiling}\n");

    let mut by_rule: BTreeMap<&str, Vec<&Violation>> = BTreeMap::new();

    for violation in &found {
        by_rule.entry(violation.rule).or_default().push(violation);
    }

    for (rule, violations) in &by_rule {
        println!("{rule} ({})", violations.len());

        for violation in violations.iter().take(12) {
            println!("  {} {}", violation.path, violation.detail);
        }

        if violations.len() > 12 {
            println!("  and {} more", violations.len() - 12);
        }

        println!();
    }

    if !WAIVED.is_empty() {
        println!("waived, with reasons:");

        for (path, reason) in WAIVED {
            println!("  {path}: {reason}");
        }

        println!();
    }

    if found.len() > ceiling {
        println!(
            "FAIL: {} violations, ceiling is {ceiling}. Move the code rather than raising it.",
            found.len()
        );

        return ExitCode::FAILURE;
    }

    println!("OK: {} violations, ceiling is {ceiling}", found.len());

    ExitCode::SUCCESS
}

#[cfg(test)]
mod tests {
    use super::*;

    fn source(relative: &str, text: &str) -> Source {
        Source {
            path: PathBuf::from(relative),
            relative: relative.to_string(),
            text: text.to_string(),
        }
    }

    #[test]
    fn a_driver_reaching_an_adapter_is_a_violation() {
        let got = layer_imports(&source(
            "src/driver/thing_driver.rs",
            "use crate::adapter::clipboard_adapter::Clipboard;\n",
        ));

        assert_eq!(got.len(), 1, "{got:?}");
        assert_eq!(got[0].rule, "layers only depend downward");
    }

    #[test]
    fn a_driver_reaching_a_controller_is_fine() {
        let got = layer_imports(&source(
            "src/driver/thing_driver.rs",
            "use crate::controller::thing_controller::Thing;\n",
        ));

        assert!(got.is_empty(), "{got:?}");
    }

    #[test]
    fn a_controller_reaching_a_driver_is_a_violation() {
        let got = layer_imports(&source(
            "src/controller/thing_controller.rs",
            "use crate::driver::thing_driver::Thing;\n",
        ));

        assert_eq!(got.len(), 1, "{got:?}");
    }

    #[test]
    fn an_adapter_may_import_anything_it_likes() {
        let got = layer_imports(&source(
            "src/adapter/thing_adapter.rs",
            "use crate::driver::thing_driver::Thing;\n",
        ));

        assert!(got.is_empty(), "{got:?}");
    }

    #[test]
    fn the_word_adapter_inside_a_string_is_not_an_import() {
        let got = layer_imports(&source(
            "src/driver/thing_driver.rs",
            "let s = \"crate::adapter::nope\";\n",
        ));

        assert!(got.is_empty(), "{got:?}");
    }

    #[test]
    fn a_long_main_is_a_violation() {
        let long = "x\n".repeat(MAIN_MAX_LINES + 1);

        assert_eq!(main_is_only_wiring(&source(MAIN_BINARY, &long)).len(), 1);
    }

    #[test]
    fn a_short_main_is_fine() {
        let short = "x\n".repeat(MAIN_MAX_LINES);

        assert!(main_is_only_wiring(&source(MAIN_BINARY, &short)).is_empty());
    }

    #[test]
    fn the_line_limit_applies_only_to_main() {
        let long = "x\n".repeat(MAIN_MAX_LINES + 500);

        assert!(main_is_only_wiring(&source("src/driver/big.rs", &long)).is_empty());
    }

    #[test]
    fn a_hand_written_fake_is_a_violation() {
        for text in [
            "struct FakeClock;\n",
            "    struct MockThing;\n",
            "struct StubRepo {}\n",
        ] {
            assert_eq!(
                hand_written_fakes(&source("src/adapter/a.rs", text)).len(),
                1,
                "{text}"
            );
        }
    }

    #[test]
    fn a_type_merely_containing_fake_is_not_a_violation() {
        let got = hand_written_fakes(&source(
            "src/adapter/a.rs",
            "struct Faker;\nstruct Remock;\n",
        ));

        assert!(got.is_empty(), "{got:?}");
    }

    #[test]
    fn the_boundary_is_the_end_of_the_word() {
        assert!(starts_a_fake("struct FakeClock;", "struct Fake"));
        assert!(starts_a_fake("struct Fake;", "struct Fake"));
        assert!(!starts_a_fake("struct Faker;", "struct Fake"));
        assert!(!starts_a_fake("struct Fakery {}", "struct Fake"));
    }

    #[test]
    fn any_comment_is_a_violation() {
        for text in ["// a note\n", "/* a block */\n", "    /// a doc\n"] {
            assert_eq!(no_comments(&source("src/a.rs", text)).len(), 1, "{text}");
        }
    }

    #[test]
    fn a_url_in_a_string_is_not_a_comment() {
        let got = no_comments(&source("src/a.rs", "let u = \"https://example.com\";\n"));

        assert!(got.is_empty(), "{got:?}");
    }

    #[test]
    fn generated_files_are_left_alone() {
        assert!(is_generated("src/config/zz_generated_config.rs"));
        assert!(!is_generated("src/config/mod.rs"));
    }

    #[test]
    fn a_module_nobody_declares_is_a_violation() {
        let sources = vec![
            source("src/controller/mod.rs", "pub mod known;\n"),
            source("src/controller/known.rs", ""),
            source("src/controller/orphan.rs", ""),
        ];

        let got = modules_are_declared(&sources);

        assert_eq!(got.len(), 1, "{got:?}");
        assert!(got[0].path.contains("orphan"), "{got:?}");
    }

    #[test]
    fn a_binary_needs_no_mod_declaration() {
        let sources = vec![
            source("src/lib.rs", ""),
            source("src/bin/poe-trader-arch.rs", ""),
        ];

        assert!(modules_are_declared(&sources).is_empty());
    }

    #[test]
    fn every_waiver_carries_a_reason() {
        for (path, reason) in WAIVED {
            assert!(
                reason.len() > 40,
                "{path} is waived without a real reason: {reason}"
            );
        }
    }

    #[test]
    fn a_waived_file_still_has_to_obey_the_comment_rule() {
        let waived = WAIVED[0].0;
        let sources = vec![source(waived, "// a note\nuse crate::adapter::x::Y;\n")];

        let found = check(&sources);

        assert!(
            found.iter().any(|v| v.rule == "no comments"),
            "a waiver must not excuse everything: {found:?}"
        );
        assert!(
            !found
                .iter()
                .any(|v| v.rule == "layers only depend downward"),
            "the waived rule should be excused: {found:?}"
        );
    }
}
