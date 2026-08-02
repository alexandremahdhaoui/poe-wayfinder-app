//! Headless price check. Same core as the overlay, no window.
//!
//! It exists so the parser and the trade query can be exercised without a
//! game, a display or a hotkey. Every conformance test runs through it.

use std::process::ExitCode;

use poe_trader_app::adapter::http_adapter::NetworkPolicy;
use poe_trader_app::config::PoeTraderCliConfig;
use poe_trader_app::logging::{Logger, Value};
use poe_trader_core::controller::parse::text_to_sections;
use poe_trader_core::types::GameVersion;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();

    let cfg = match PoeTraderCliConfig::load(&args) {
        Ok(cfg) => cfg,
        Err(err) => {
            eprintln!("poe-trader-cli: loading config: {err}");

            return ExitCode::FAILURE;
        }
    };

    let log = Logger::new(&cfg.log_level, "poe-trader-cli");

    let Some(game) = GameVersion::parse(&cfg.game) else {
        log.error("unknown game", &[("game", Value::Str(cfg.game.clone()))]);

        return ExitCode::FAILURE;
    };

    let policy = NetworkPolicy::new(
        cfg.network_enabled,
        cfg.block_unlisted_hosts,
        &cfg.allowed_hosts,
    );

    log.info(
        "network policy",
        &[
            ("enabled", Value::Bool(cfg.network_enabled)),
            ("hosts", Value::Str(policy.allowed_hosts().join(","))),
        ],
    );

    if cfg.item_file.is_empty() {
        log.error(
            "no item source",
            &[("hint", Value::Str("set --item-file".into()))],
        );

        return ExitCode::FAILURE;
    }

    let text = match std::fs::read_to_string(&cfg.item_file) {
        Ok(text) => text,
        Err(err) => {
            log.error(
                "reading item file",
                &[
                    ("path", Value::Str(cfg.item_file.clone())),
                    ("error", Value::Str(err.to_string())),
                ],
            );

            return ExitCode::FAILURE;
        }
    };

    let sections = text_to_sections(&text);

    log.info(
        "parsed item text",
        &[
            ("game", Value::Str(game.as_str().to_string())),
            ("sections", Value::Int(sections.len() as i64)),
        ],
    );

    log.warn("the parser pipeline is not implemented yet", &[]);

    ExitCode::SUCCESS
}
