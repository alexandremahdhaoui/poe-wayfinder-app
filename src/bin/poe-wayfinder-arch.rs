use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

const MAIN_BINARY: &str = "src/bin/poe-wayfinder.rs";
const MAIN_MAX_LINES: usize = 220;

const SIBLING_CRATES: &[&str] = &["poe-wayfinder-core", "poe-wayfinder-data"];

const WAIVED: &[(&str, &str)] = &[
    (
        "src/driver/cli_driver.rs",
        "self tests and diagnostics read adapters directly on purpose, because their whole \
         job is to report what one adapter sees before any controller exists to ask",
    ),
    (
        "src/driver/overlay_loop/wiring.rs",
        "the composition root builds concrete adapters and hands them to controllers, which \
         is main's job and the one place the layering is meant to be crossed",
    ),
];

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
        eprintln!("poe-wayfinder-arch: no src directory at {}", src.display());

        return ExitCode::FAILURE;
    }

    let mut sources = Vec::new();
    collect(&src, &root, &mut sources);

    let mut everything = Vec::new();

    collect(&src, &root, &mut everything);

    for sibling in SIBLING_CRATES {
        let dir = root.join("..").join(sibling).join("src");

        if dir.is_dir() {
            collect(&dir, &root, &mut everything);
        }
    }

    report(&sources, &everything, ceiling)
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

const UNWIRED_EXEMPT: &[&str] = &["new", "default", "fmt", "from", "parse", "as_str"];

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

fn public_functions(text: &str) -> Vec<String> {
    let mut out = Vec::new();

    for line in production(text).lines() {
        let trimmed = line.trim_start();

        let Some(rest) = trimmed.strip_prefix("pub fn ") else {
            continue;
        };

        let name: String = rest
            .chars()
            .take_while(|c| c.is_alphanumeric() || *c == '_')
            .collect();

        if name.is_empty() || UNWIRED_EXEMPT.contains(&name.as_str()) {
            continue;
        }

        out.push(name);
    }

    out
}

fn calls_in(text: &str, name: &str) -> usize {
    production(text)
        .lines()
        .filter(|line| {
            let trimmed = line.trim_start();

            !trimmed.starts_with(&format!("pub fn {name}"))
                && !trimmed.starts_with(&format!("fn {name}"))
        })
        .map(outside_quotes)
        .filter(|line| mentions(line, name))
        .count()
}

fn outside_quotes(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut quoted = false;
    let mut escaped = false;

    for character in line.chars() {
        if escaped {
            escaped = false;

            continue;
        }

        match character {
            '\\' if quoted => escaped = true,
            '"' => quoted = !quoted,
            _ if quoted => {}
            _ => out.push(character),
        }
    }

    out
}

fn mentions(line: &str, name: &str) -> bool {
    let bytes = line.as_bytes();
    let mut from = 0;

    while let Some(at) = line[from..].find(name) {
        let start = from + at;
        let end = start + name.len();

        let before_ok = start == 0 || !is_word_byte(bytes[start - 1]);
        let after_ok = end >= bytes.len() || !is_word_byte(bytes[end]);

        if before_ok && after_ok {
            return true;
        }

        from = end;
    }

    false
}

fn is_word_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}

const NOT_A_CALLER: &[&str] = &[
    "src/bin/poe-wayfinder-uiparity.rs",
    "src/bin/poe-wayfinder-parity.rs",
];

pub struct Wiring {
    pub wired: usize,
    pub total: usize,
}

impl Wiring {
    fn percent(&self) -> f64 {
        match self.total {
            0 => 100.0,
            total => (self.wired as f64 / total as f64) * 100.0,
        }
    }
}

fn wiring(everything: &[Source]) -> Wiring {
    let mut wired = 0;
    let mut total = 0;

    for source in everything {
        if is_generated(&source.relative) {
            continue;
        }

        for name in public_functions(&source.text) {
            total += 1;

            let calls: usize = everything
                .iter()
                .filter(|other| !is_generated(&other.relative))
                .filter(|other| !NOT_A_CALLER.contains(&other.relative.as_str()))
                .map(|other| calls_in(&other.text, &name))
                .sum();

            if calls > 0 {
                wired += 1;
            }
        }
    }

    Wiring { wired, total }
}

fn test_counts(everything: &[Source]) -> (usize, usize) {
    let mut tests = 0;
    let mut untested = 0;

    for source in everything {
        if is_generated(&source.relative) {
            continue;
        }

        let here = source.text.matches("    #[test]").count();

        tests += here;

        if here == 0 && !public_functions(&source.text).is_empty() {
            untested += 1;
        }
    }

    (tests, untested)
}

fn no_unwired_public_functions(everything: &[Source]) -> Vec<Violation> {
    let mut found = Vec::new();

    for source in everything {
        if is_generated(&source.relative) {
            continue;
        }

        for name in public_functions(&source.text) {
            let calls: usize = everything
                .iter()
                .filter(|other| !is_generated(&other.relative))
                .filter(|other| !NOT_A_CALLER.contains(&other.relative.as_str()))
                .map(|other| calls_in(&other.text, &name))
                .sum();

            if calls > 0 {
                continue;
            }

            found.push(Violation {
                rule: "no unwired code",
                path: source.relative.clone(),
                detail: format!("{name} is public and no production code calls it"),
            });
        }
    }

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

fn forbidden_imports(relative: &str) -> Vec<(&'static str, &'static str)> {
    if relative.starts_with("src/driver/") {
        return vec![("crate::adapter::", "a driver reaches an adapter")];
    }

    if relative.starts_with("src/controller/") {
        return vec![("crate::driver::", "a controller reaches a driver")];
    }

    if relative.starts_with("src/adapter/") {
        return vec![
            ("crate::driver::", "an adapter reaches a driver"),
            ("crate::controller::", "an adapter reaches a controller"),
            ("crate::adapter::", "an adapter reaches another adapter"),
        ];
    }

    Vec::new()
}

fn layer_imports(source: &Source) -> Vec<Violation> {
    let forbidden = forbidden_imports(&source.relative);

    if forbidden.is_empty() {
        return Vec::new();
    }

    let own = format!(
        "crate::adapter::{}",
        source
            .path
            .file_stem()
            .and_then(|f| f.to_str())
            .unwrap_or_default()
    );

    source
        .text
        .lines()
        .enumerate()
        .flat_map(|(n, line)| {
            let code = without_string_literals(line);

            forbidden
                .iter()
                .filter(|(prefix, _)| code.contains(*prefix) && !code.contains(&own))
                .map(|(_, why)| Violation {
                    rule: "layers only depend downward",
                    path: source.relative.clone(),
                    detail: format!("line {}: {why}: {}", n + 1, line.trim()),
                })
                .collect::<Vec<_>>()
        })
        .collect()
}

fn without_string_literals(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut inside = false;
    let mut escaped = false;

    for c in line.chars() {
        if escaped {
            escaped = false;

            continue;
        }

        match c {
            '\\' if inside => escaped = true,
            '"' => inside = !inside,
            _ if !inside => out.push(c),
            _ => {}
        }
    }

    out
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

fn report(sources: &[Source], everything: &[Source], ceiling: usize) -> ExitCode {
    let mut found = check(sources);
    found.extend(no_unwired_public_functions(everything));

    let wiring = wiring(everything);
    let (tests, untested) = test_counts(everything);

    println!("poe-wayfinder architecture report\n");
    println!("  files scanned  : {}", everything.len());
    println!(
        "  wired          : {:.1}%  ({} of {} public functions have a caller)",
        wiring.percent(),
        wiring.wired,
        wiring.total
    );
    println!("  tests          : {tests}");
    println!("  files with public code and no test: {untested}");
    println!("  violations     : {}", found.len());
    println!("  ceiling        : {ceiling}\n");

    let mut by_rule: BTreeMap<&str, Vec<&Violation>> = BTreeMap::new();

    for violation in &found {
        by_rule.entry(violation.rule).or_default().push(violation);
    }

    for (rule, violations) in &by_rule {
        println!("{rule} ({})", violations.len());

        for violation in violations.iter().take(120) {
            println!("  {} {}", violation.path, violation.detail);
        }

        if violations.len() > 120 {
            println!("  and {} more", violations.len() - 120);
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
    fn an_adapter_reaching_a_driver_is_a_violation() {
        let got = layer_imports(&source(
            "src/adapter/thing_adapter.rs",
            "use crate::driver::thing_driver::Thing;",
        ));

        assert_eq!(got.len(), 1, "{got:?}");
    }

    #[test]
    fn an_adapter_reaching_a_controller_is_a_violation() {
        let got = layer_imports(&source(
            "src/adapter/clock_adapter.rs",
            "use crate::controller::price_check_controller::Clock;",
        ));

        assert_eq!(got.len(), 1, "{got:?}");
    }

    #[test]
    fn an_adapter_reaching_another_adapter_is_a_violation() {
        let got = layer_imports(&source(
            "src/adapter/clock_adapter.rs",
            "use crate::adapter::rate_limit_adapter::Millis;",
        ));

        assert_eq!(got.len(), 1, "{got:?}");
    }

    #[test]
    fn an_adapter_may_refer_to_its_own_module() {
        let got = layer_imports(&source(
            "src/adapter/clock_adapter.rs",
            "use crate::adapter::clock_adapter::SystemClock;",
        ));

        assert!(got.is_empty(), "{got:?}");
    }

    #[test]
    fn an_adapter_may_import_core() {
        let got = layer_imports(&source(
            "src/adapter/thing_adapter.rs",
            "use poe_wayfinder_core::types::GameVersion;",
        ));

        assert!(got.is_empty(), "{got:?}");
    }

    #[test]
    fn a_driver_naming_an_adapter_by_full_path_outside_a_use_line_is_still_a_violation() {
        let got = layer_imports(&source(
            "src/driver/thing_driver.rs",
            "fn look(found: Option<crate::adapter::game_window_adapter::GameWindow>) {}\n",
        ));

        assert_eq!(got.len(), 1, "{got:?}");
        assert_eq!(got[0].rule, "layers only depend downward");
    }

    #[test]
    fn a_driver_naming_an_adapter_error_type_in_a_match_arm_is_still_a_violation() {
        let got = layer_imports(&source(
            "src/driver/thing_driver.rs",
            "            Err(crate::adapter::clipboard_adapter::ClipboardError::Empty) => None,\n",
        ));

        assert_eq!(got.len(), 1, "{got:?}");
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
            source("src/bin/poe-wayfinder-arch.rs", ""),
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
    #[test]
    fn code_after_a_test_module_is_still_production_code() {
        let body =
            "fn early() {}\n#[cfg(test)]\nmod tests {\n    fn helper() {}\n}\nfn late() {}\n";

        let kept = production(body);

        assert!(kept.contains("fn early"));
        assert!(
            kept.contains("fn late"),
            "a truncating stripper hides the rest of the file"
        );
        assert!(!kept.contains("fn helper"));
    }

    #[test]
    fn a_nested_brace_inside_a_test_module_does_not_end_it_early() {
        let body = "#[cfg(test)]\nmod tests {\n    fn a() {\n        if x { y }\n    }\n}\nfn after() {}\n";

        let kept = production(body);

        assert!(!kept.contains("fn a("));
        assert!(kept.contains("fn after"));
    }

    #[test]
    fn a_file_with_no_tests_is_left_whole() {
        let body = "fn one() {}\nfn two() {}\n";

        assert_eq!(production(body), body);
    }

    #[test]
    fn a_definition_does_not_count_as_a_call_to_itself() {
        assert_eq!(calls_in("pub fn lonely() {}\n", "lonely"), 0);
    }

    #[test]
    fn a_use_of_a_function_counts_as_a_call() {
        assert_eq!(
            calls_in("pub fn lonely() {}\nfn other() { lonely(); }\n", "lonely"),
            1
        );
    }

    #[test]
    fn importing_a_function_counts_as_using_it() {
        assert_eq!(calls_in("use super::{lonely, other};\n", "lonely"), 1);
    }

    #[test]
    fn a_longer_name_that_contains_the_short_one_is_not_a_call() {
        assert_eq!(calls_in("fn other() { lonely_helper(); }\n", "lonely"), 0);
    }

    #[test]
    fn a_call_inside_a_test_does_not_keep_a_function_alive() {
        let body = "pub fn lonely() {}\n#[cfg(test)]\nmod tests {\n    fn t() { lonely(); }\n}\n";

        assert_eq!(calls_in(body, "lonely"), 0);
    }

    #[test]
    fn a_table_of_names_is_not_treated_as_a_caller() {
        assert!(
            NOT_A_CALLER.contains(&"src/bin/poe-wayfinder-uiparity.rs"),
            "the capability catalogue names every symbol it measures"
        );
        assert!(
            NOT_A_CALLER.contains(&"src/bin/poe-wayfinder-parity.rs"),
            "the alias table pairs an upstream name with ours, and counting it \
             hid fifty one dead functions behind a claim of 100 percent"
        );
    }

    #[test]
    fn a_name_inside_a_log_message_is_not_a_call() {
        let logged = r#"log.info("watching the hotkeys", &[("watching", value)]);"#;

        assert_eq!(calls_in(logged, "watching"), 0);
    }

    #[test]
    fn a_real_call_beside_a_string_is_still_a_call() {
        let line = r#"log.info("watching", &[]); watching();"#;

        assert_eq!(calls_in(line, "watching"), 1);
    }

    #[test]
    fn an_escaped_quote_does_not_swallow_the_rest_of_the_line() {
        let line = r#"println!("say \"hi\""); wired();"#;

        assert_eq!(calls_in(line, "wired"), 1);
    }

    #[test]
    fn a_public_function_is_found_by_name() {
        let names = public_functions("pub fn wired() {}\nfn private() {}\n");

        assert_eq!(names, vec!["wired".to_string()]);
    }

    #[test]
    fn a_constructor_is_exempt_because_every_type_has_one() {
        assert!(public_functions("pub fn new() -> Self { Self }\n").is_empty());
    }
}
