//! Headless price check. Same core as the overlay, no window.
//!
//! It exists so the parser and the trade query can be exercised without a
//! game, a display or a hotkey. Every conformance test runs through it.

use std::process::ExitCode;

use poe_trader_app::adapter::game_data_adapter::GameTables;
use poe_trader_app::adapter::http_adapter::NetworkPolicy;
use poe_trader_app::adapter::query_json_adapter::{to_exchange_json, to_json};
use poe_trader_app::config::PoeTraderCliConfig;
use poe_trader_app::logging::{Logger, Value};
use poe_trader_core::controller::bulk::Endpoint;
use poe_trader_core::controller::price_check::{price_check, PriceCheckOptions};
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

    let data = match GameTables::load(std::path::Path::new(&cfg.data_dir)) {
        Ok(data) => data,
        Err(err) => {
            log.error(
                "loading game data",
                &[
                    ("data_dir", Value::Str(cfg.data_dir.clone())),
                    ("error", Value::Str(err.to_string())),
                ],
            );

            return ExitCode::FAILURE;
        }
    };

    log.info(
        "loaded game data",
        &[
            ("stats", Value::Int(data.stat_count() as i64)),
            ("item_names", Value::Int(data.item_name_count() as i64)),
        ],
    );

    let checked = match price_check(&text, &data, PriceCheckOptions::new(game)) {
        Ok(checked) => checked,
        Err(err) => {
            log.error(
                "parsing item text",
                &[
                    ("path", Value::Str(cfg.item_file.clone())),
                    ("error", Value::Str(err.to_string())),
                ],
            );

            return ExitCode::FAILURE;
        }
    };

    let item = &checked.item;

    log.info(
        "parsed item",
        &[
            ("game", Value::Str(game.as_str().to_string())),
            (
                "rarity",
                Value::Str(
                    item.rarity
                        .map(|r| r.as_str().to_string())
                        .unwrap_or_default(),
                ),
            ),
            (
                "category",
                Value::Str(
                    item.category
                        .map(|c| c.as_str().to_string())
                        .unwrap_or_default(),
                ),
            ),
            (
                "item_level",
                Value::Int(item.item_level.unwrap_or(0) as i64),
            ),
            ("quality", Value::Int(item.quality.unwrap_or(0) as i64)),
            ("corrupted", Value::Bool(item.is_corrupted)),
            ("unidentified", Value::Bool(item.is_unidentified)),
            ("modifiers", Value::Int(item.modifiers.len() as i64)),
            (
                "stat_filters",
                Value::Int(checked.stat_filter_count() as i64),
            ),
            (
                "unknown_modifiers",
                Value::Int(item.unknown_modifiers.len() as i64),
            ),
        ],
    );

    // An unknown modifier means our data is older than the game. A price built
    // from a partly understood item is wrong in a way the user cannot see.
    for unknown in &item.unknown_modifiers {
        log.warn(
            "modifier not recognised",
            &[("text", Value::Str(unknown.text.clone()))],
        );
    }

    // A currency goes to the exchange endpoint, which takes a different body
    // shape. Printing the search body for one would show a request the overlay
    // is not going to send.
    let body = match (checked.endpoint, &checked.trade_tag) {
        (Endpoint::Exchange, Some(tag)) => to_exchange_json(tag, &[], checked.query.status),
        _ => to_json(&checked.query),
    };

    match serde_json::to_string_pretty(&body) {
        Ok(text) => println!("{text}"),
        Err(err) => {
            log.error(
                "serialising the trade query",
                &[("error", Value::Str(err.to_string()))],
            );

            return ExitCode::FAILURE;
        }
    }

    ExitCode::SUCCESS
}
