//! Builds data/*.ndjson.
//!
//! This replaces the reference data pipeline, which is 7623 lines of Python.
//! It pulls the trade stat and item tables from the official API and joins
//! them against the extracted game tables.

use std::process::ExitCode;

use poe_trader_app::adapter::http_adapter::NetworkPolicy;
use poe_trader_app::config::PoeTraderDatagenConfig;
use poe_trader_app::logging::{Logger, Value};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();

    let cfg = match PoeTraderDatagenConfig::load(&args) {
        Ok(cfg) => cfg,
        Err(err) => {
            eprintln!("poe-trader-datagen: loading config: {err}");

            return ExitCode::FAILURE;
        }
    };

    let log = Logger::new(&cfg.log_level, "poe-trader-datagen");

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

    if let Err(err) = policy.check(&cfg.trade_base_url) {
        log.error(
            "trade_base_url is refused by the network policy",
            &[
                ("url", Value::Str(cfg.trade_base_url.clone())),
                ("error", Value::Str(err.to_string())),
            ],
        );

        return ExitCode::FAILURE;
    }

    log.info(
        "startup",
        &[
            ("game", Value::Str(cfg.game.clone())),
            ("tables_dir", Value::Str(cfg.tables_dir.clone())),
            ("out_dir", Value::Str(cfg.out_dir.clone())),
        ],
    );

    log.warn("the data pipeline is not implemented yet", &[]);

    ExitCode::SUCCESS
}
