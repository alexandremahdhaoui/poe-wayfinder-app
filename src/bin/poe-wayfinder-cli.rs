use std::process::ExitCode;
use std::time::Duration;

use poe_wayfinder_app::adapter::config_store_adapter;
use poe_wayfinder_app::adapter::game_data_adapter::GameTables;
use poe_wayfinder_app::adapter::http_adapter::NetworkPolicy;
use poe_wayfinder_app::adapter::query_json_adapter::{to_exchange_json, to_json};
use poe_wayfinder_app::config::PoeWayfinderCliConfig;
use poe_wayfinder_app::logging::{Logger, Value};
use poe_wayfinder_core::controller::bulk::Endpoint;
use poe_wayfinder_core::controller::price_check::{price_check, PriceCheckOptions};
use poe_wayfinder_core::types::GameVersion;

fn main() -> ExitCode {
    let should_send = std::env::args().any(|a| a == "--send");

    let args: Vec<String> = std::env::args().skip(1).filter(|a| a != "--send").collect();

    let cfg = match PoeWayfinderCliConfig::load(&args) {
        Ok(cfg) => cfg,
        Err(err) => {
            eprintln!("poe-wayfinder-cli: loading config: {err}");

            return ExitCode::FAILURE;
        }
    };

    let log = Logger::new(&cfg.log_level, "poe-wayfinder-cli");

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
                    (
                        "error",
                        Value::Str(poe_wayfinder_app::util::error_chain::render(&err)),
                    ),
                ],
            );

            return ExitCode::FAILURE;
        }
    };

    let config_dir = config_store_adapter::resolve_dir("");

    let (data, origin) = match GameTables::resolve(&cfg.data_dir, &config_dir, game) {
        Ok(found) => found,
        Err(err) => {
            log.error(
                "loading game data",
                &[
                    ("data_dir", Value::Str(cfg.data_dir.clone())),
                    (
                        "error",
                        Value::Str(poe_wayfinder_app::util::error_chain::render(&err)),
                    ),
                ],
            );

            return ExitCode::FAILURE;
        }
    };

    log.info(
        "loaded game data",
        &[
            ("origin", Value::Str(origin.as_str().to_string())),
            ("stats", Value::Int(data.stat_count() as i64)),
            ("item_names", Value::Int(data.item_name_count() as i64)),
        ],
    );

    let checked = match price_check(&text, &data, &PriceCheckOptions::new(game)) {
        Ok(checked) => checked,
        Err(err) => {
            log.error(
                "parsing item text",
                &[
                    ("path", Value::Str(cfg.item_file.clone())),
                    (
                        "error",
                        Value::Str(poe_wayfinder_app::util::error_chain::render(&err)),
                    ),
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

    for unknown in &item.unknown_modifiers {
        log.warn(
            "modifier not recognised",
            &[("text", Value::Str(unknown.text.clone()))],
        );
    }

    let body = match (checked.endpoint, &checked.trade_tag) {
        (Endpoint::Exchange, Some(tag)) => to_exchange_json(tag, &[], checked.query.status),
        _ => to_json(&checked.query, game),
    };

    match serde_json::to_string_pretty(&body) {
        Ok(text) => println!("{text}"),
        Err(err) => {
            log.error(
                "serialising the trade query",
                &[(
                    "error",
                    Value::Str(poe_wayfinder_app::util::error_chain::render(&err)),
                )],
            );

            return ExitCode::FAILURE;
        }
    }

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
                &[(
                    "error",
                    Value::Str(poe_wayfinder_app::util::error_chain::render(&err)),
                )],
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

async fn send(
    cfg: &PoeWayfinderCliConfig,
    game: GameVersion,
    checked: &poe_wayfinder_core::controller::price_check::PriceCheck,
    body: &serde_json::Value,
    log: &Logger,
) -> Result<(), String> {
    use poe_wayfinder_app::adapter::http_adapter::{HttpAdapter, HttpClient};
    use poe_wayfinder_app::adapter::rate_limit_adapter::LimiterSet;
    use poe_wayfinder_app::adapter::trade_api_adapter::TradeUrls;
    use poe_wayfinder_app::controller::price_check_controller::{
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

    let wait = limits.wait_for(now_millis());

    if wait > 0 {
        log.info(
            "waiting for the rate limiter",
            &[("ms", Value::Int(wait as i64))],
        );
        std::thread::sleep(Duration::from_millis(wait));
    }

    limits.borrow(now_millis());

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

fn report_price(
    listings: &[poe_wayfinder_app::controller::price_check_controller::Listing],
    log: &Logger,
) {
    use poe_wayfinder_app::controller::price_check_controller::suggested_price;

    let Some((amount, currency)) = suggested_price(listings) else {
        log.warn("the listings carry no price to average", &[]);

        return;
    };

    let shown = poe_wayfinder_core::controller::money::price(amount, &currency);

    let mut fields = vec![
        ("amount", Value::Str(shown.amount)),
        ("currency", Value::Str(shown.currency)),
    ];

    if let Some(inverted) = shown.inverted {
        fields.push(("also", Value::Str(inverted)));
    }

    log.info("suggested price", &fields);
}

fn now_millis() -> u64 {
    use std::sync::OnceLock;
    use std::time::Instant;

    static START: OnceLock<Instant> = OnceLock::new();

    START.get_or_init(Instant::now).elapsed().as_millis() as u64
}
