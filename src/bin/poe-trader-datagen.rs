//! Builds data/*.ndjson.
//!
//! This replaces the reference data pipeline, which is 7623 lines of Python.
//! It pulls the trade stat and item tables from the official API and joins
//! them against the extracted game tables.

use std::path::Path;
use std::process::ExitCode;
use std::time::Duration;

use poe_trader_app::adapter::http_adapter::{HttpAdapter, HttpClient, NetworkPolicy};
use poe_trader_app::adapter::trade_api_adapter::TradeUrls;
use poe_trader_app::config::PoeTraderDatagenConfig;
use poe_trader_app::controller::datagen_controller::{
    build_items, build_stats, build_trade_tags, item_to_ndjson, stat_to_ndjson,
};
use poe_trader_app::logging::{Logger, Value};
use poe_trader_core::types::GameVersion;

fn main() -> ExitCode {
    // The builder is one sequential pass over two endpoints. A single threaded
    // runtime keeps the binary small and the ordering obvious.
    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(err) => {
            eprintln!("poe-trader-datagen: starting the runtime: {err}");

            return ExitCode::FAILURE;
        }
    };

    runtime.block_on(run())
}

async fn run() -> ExitCode {
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

    let Some(game) = GameVersion::parse(&cfg.game) else {
        log.error("unknown game", &[("game", Value::Str(cfg.game.clone()))]);

        return ExitCode::FAILURE;
    };

    log.info(
        "startup",
        &[
            ("game", Value::Str(game.as_str().to_string())),
            ("tables_dir", Value::Str(cfg.tables_dir.clone())),
            ("out_dir", Value::Str(cfg.out_dir.clone())),
        ],
    );

    let http = match HttpAdapter::with_user_agent(policy, Duration::from_secs(60), &cfg.user_agent)
    {
        Ok(http) => http,
        Err(err) => {
            log.error(
                "building http client",
                &[("error", Value::Str(err.to_string()))],
            );

            return ExitCode::FAILURE;
        }
    };

    let urls = TradeUrls::new(&cfg.trade_base_url, game);

    // Two tables, fetched one after the other. There is no rate limit on the
    // data endpoints, and doing them in order keeps a failure obvious.
    let stats_body = match fetch(&http, &urls.data("stats"), &log).await {
        Some(body) => body,
        None => return ExitCode::FAILURE,
    };

    let items_body = match fetch(&http, &urls.data("items"), &log).await {
        Some(body) => body,
        None => return ExitCode::FAILURE,
    };

    // The bulk trading tags. The exchange endpoint knows a currency by a short
    // id rather than by its name, and without this every currency price check
    // goes to the search endpoint and returns the few individual listings
    // instead of the market rate.
    let static_body = match fetch(&http, &urls.data("static"), &log).await {
        Some(body) => body,
        None => return ExitCode::FAILURE,
    };

    let trade_tags = match build_trade_tags(&static_body) {
        Ok(tags) => tags,
        Err(err) => {
            log.error(
                "building trade tags",
                &[("error", Value::Str(err.to_string()))],
            );

            return ExitCode::FAILURE;
        }
    };

    let stats = match build_stats(&stats_body) {
        Ok(stats) => stats,
        Err(err) => {
            log.error("building stats", &[("error", Value::Str(err.to_string()))]);

            return ExitCode::FAILURE;
        }
    };

    let items = match build_items(&items_body, &trade_tags) {
        Ok(items) => items,
        Err(err) => {
            log.error("building items", &[("error", Value::Str(err.to_string()))]);

            return ExitCode::FAILURE;
        }
    };

    let out_dir = Path::new(&cfg.out_dir);

    if let Err(err) = std::fs::create_dir_all(out_dir) {
        log.error(
            "creating the output directory",
            &[
                ("out_dir", Value::Str(cfg.out_dir.clone())),
                ("error", Value::Str(err.to_string())),
            ],
        );

        return ExitCode::FAILURE;
    }

    let stats_lines: Vec<String> = stats.iter().map(stat_to_ndjson).collect();
    let items_lines: Vec<String> = items.iter().map(item_to_ndjson).collect();

    for (name, lines) in [
        ("stats.ndjson", &stats_lines),
        ("items.ndjson", &items_lines),
    ] {
        let path = out_dir.join(name);
        // A trailing newline so the file ends cleanly. The loader skips blank
        // lines, so this costs nothing and every text tool expects it.
        let body = format!("{}\n", lines.join("\n"));

        if let Err(err) = std::fs::write(&path, body) {
            log.error(
                "writing the data file",
                &[
                    ("path", Value::Str(path.display().to_string())),
                    ("error", Value::Str(err.to_string())),
                ],
            );

            return ExitCode::FAILURE;
        }

        log.info(
            "wrote data file",
            &[
                ("path", Value::Str(path.display().to_string())),
                ("records", Value::Int(lines.len() as i64)),
            ],
        );
    }

    let with_category = items.iter().filter(|i| i.category.is_some()).count();

    log.info(
        "category coverage",
        &[
            ("with_category", Value::Int(with_category as i64)),
            ("total", Value::Int(items.len() as i64)),
        ],
    );

    // Said plainly rather than left for a user to discover. The trade API
    // groups items as accessory, armour and weapon, so only the groups that
    // are already one category resolve, plus accessories, whose names are
    // completely regular. Weapon and armour categories, roll ranges, quality
    // scaling and modifier tiers all come from the game's own data bundles.
    log.warn(
        "weapon and armour categories, roll ranges and modifier tiers are not built here. They come from the game bundles and are vendored in poe-trader-data/tables.",
        &[],
    );

    ExitCode::SUCCESS
}

/// Fetch one table, logging what failed.
async fn fetch(http: &HttpAdapter, url: &str, log: &Logger) -> Option<String> {
    let response = match http.get(url, &[("accept", "application/json")]).await {
        Ok(response) => response,
        Err(err) => {
            log.error(
                "fetching a data table",
                &[
                    ("url", Value::Str(url.to_string())),
                    ("error", Value::Str(err.to_string())),
                ],
            );

            return None;
        }
    };

    if !(200..300).contains(&response.status) {
        log.error(
            "a data table answered with a failure status",
            &[
                ("url", Value::Str(url.to_string())),
                ("status", Value::Int(i64::from(response.status))),
            ],
        );

        return None;
    }

    log.info(
        "fetched a data table",
        &[
            ("url", Value::Str(url.to_string())),
            ("bytes", Value::Int(response.body.len() as i64)),
        ],
    );

    Some(response.body)
}
