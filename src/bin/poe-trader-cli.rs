//! Headless price check. Same core as the overlay, no window.
//!
//! It exists so the parser and the trade query can be exercised without a
//! game, a display or a hotkey. Every conformance test runs through it.

use std::process::ExitCode;
use std::time::Duration;

use poe_trader_app::adapter::game_data_adapter::GameTables;
use poe_trader_app::adapter::http_adapter::NetworkPolicy;
use poe_trader_app::adapter::query_json_adapter::{to_exchange_json, to_json};
use poe_trader_app::config::PoeTraderCliConfig;
use poe_trader_app::logging::{Logger, Value};
use poe_trader_core::controller::bulk::Endpoint;
use poe_trader_core::controller::price_check::{price_check, PriceCheckOptions};
use poe_trader_core::types::GameVersion;

fn main() -> ExitCode {
    // Taken out before the config loader sees it. The loader is generated from
    // the spec and refuses a flag it does not know, and this one is a switch
    // for this binary rather than a configuration value.
    let should_send = std::env::args().any(|a| a == "--send");

    let args: Vec<String> = std::env::args().skip(1).filter(|a| a != "--send").collect();

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

    // Printing the body proves the query was built. It does not prove the
    // trade site accepts it, and a query the site rejects looks identical from
    // here.
    //
    // Off by default because it is a real request against GGG's servers, and
    // every run of the test suite firing one would be rude at best.
    if !should_send {
        return ExitCode::SUCCESS;
    }

    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(err) => {
            log.error(
                "starting the runtime",
                &[("error", Value::Str(err.to_string()))],
            );

            return ExitCode::FAILURE;
        }
    };

    match runtime.block_on(send(&cfg, game, &checked, &body, &log)) {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            log.error("sending the search", &[("error", Value::Str(message))]);

            ExitCode::FAILURE
        }
    }
}

/// Run the search for real and report what came back.
///
/// The whole chain the overlay runs, minus the clipboard: rate limiter, the
/// one allowed socket, the search, the fetch and the price. The clipboard is
/// the only part that needs a game.
async fn send(
    cfg: &PoeTraderCliConfig,
    game: GameVersion,
    checked: &poe_trader_core::controller::price_check::PriceCheck,
    body: &serde_json::Value,
    log: &Logger,
) -> Result<(), String> {
    use poe_trader_app::adapter::http_adapter::{HttpAdapter, HttpClient};
    use poe_trader_app::adapter::rate_limit_adapter::LimiterSet;
    use poe_trader_app::adapter::trade_api_adapter::TradeUrls;
    use poe_trader_app::controller::price_check_controller::{
        read_exchange_listings, read_listings, read_search_result,
    };

    let policy = NetworkPolicy::new(
        cfg.network_enabled,
        cfg.block_unlisted_hosts,
        &cfg.allowed_hosts,
    );

    let http = HttpAdapter::with_user_agent(policy, Duration::from_secs(30), &cfg.user_agent)
        .map_err(|e| format!("building the http client: {e}"))?;

    let urls = TradeUrls::new(&cfg.trade_base_url, game);
    let mut limits = LimiterSet::conservative();

    let url = match checked.endpoint {
        Endpoint::Exchange => urls.exchange(&cfg.league),
        Endpoint::Search => urls.search(&cfg.league),
    };

    let text = serde_json::to_string(body).map_err(|e| format!("serialising the body: {e}"))?;

    // The limiter is not optional. GGG bans for violations, and this is a real
    // request to their servers.
    let wait = limits.wait_for(now_millis());

    if wait > 0 {
        log.info(
            "waiting for the rate limiter",
            &[("ms", Value::Int(wait as i64))],
        );
        std::thread::sleep(Duration::from_millis(wait));
    }

    limits.borrow(now_millis());

    // No cookie. The search endpoint answers an unauthenticated request, which
    // is why the overlay no longer demands a session.
    let headers = [("accept", "application/json")];

    let response = http
        .post_json(&url, &headers, &text)
        .await
        .map_err(|e| format!("{e}"))?;

    limits.adjust(
        &response.headers,
        cfg.api_latency_seconds.max(0) as u32,
        now_millis(),
    );

    let found = read_search_result(&response).map_err(|e| format!("{e}"))?;

    log.info(
        "search returned",
        &[
            ("url", Value::Str(url)),
            ("total", Value::Int(found.total as i64)),
            ("id", Value::Str(found.id.clone())),
        ],
    );

    // The exchange endpoint already sent the listings. Only the search
    // endpoint answers with ids that have to be fetched.
    if checked.endpoint == Endpoint::Exchange {
        let listings = read_exchange_listings(&response).map_err(|e| format!("{e}"))?;

        log.info(
            "read exchange offers",
            &[("count", Value::Int(listings.len() as i64))],
        );

        report_price(&listings, log);

        return Ok(());
    }

    if found.result.is_empty() {
        log.warn("no listings matched. Nothing to price.", &[]);

        return Ok(());
    }

    // Only the first batch. A price is the median of the cheapest few and
    // fetching every page would spend the rate limit for no better answer.
    let batch: Vec<String> = found.result.iter().take(10).cloned().collect();

    let wait = limits.wait_for(now_millis());

    if wait > 0 {
        std::thread::sleep(Duration::from_millis(wait));
    }

    limits.borrow(now_millis());

    let response = http
        .get(&urls.fetch(&batch, &found.id), &headers)
        .await
        .map_err(|e| format!("{e}"))?;

    let listings = read_listings(&response).map_err(|e| format!("{e}"))?;

    log.info(
        "fetched listings",
        &[("count", Value::Int(listings.len() as i64))],
    );

    report_price(&listings, log);

    Ok(())
}

/// Log what the listings suggest.
fn report_price(
    listings: &[poe_trader_app::controller::price_check_controller::Listing],
    log: &Logger,
) {
    use poe_trader_app::controller::price_check_controller::suggested_price;

    let Some((amount, currency)) = suggested_price(listings) else {
        log.warn("the listings carry no price to average", &[]);

        return;
    };

    // Rounded before it is shown. The raw rate for a Divine in Mirrors comes
    // out as 0.0007201155913700063, which is true and unreadable.
    let shown = poe_trader_core::controller::money::price(amount, &currency);

    let mut fields = vec![
        ("amount", Value::Str(shown.amount)),
        ("currency", Value::Str(shown.currency)),
    ];

    // A rate below a hundredth is quoted the other way up, which is how the
    // game's economy is actually spoken about.
    if let Some(inverted) = shown.inverted {
        fields.push(("also", Value::Str(inverted)));
    }

    log.info("suggested price", &fields);
}

/// Milliseconds since the process started.
fn now_millis() -> u64 {
    use std::sync::OnceLock;
    use std::time::Instant;

    static START: OnceLock<Instant> = OnceLock::new();

    START.get_or_init(Instant::now).elapsed().as_millis() as u64
}
