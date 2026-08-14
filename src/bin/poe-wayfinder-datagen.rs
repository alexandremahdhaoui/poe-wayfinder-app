use std::path::Path;
use std::process::ExitCode;
use std::time::Duration;

use poe_wayfinder_app::adapter::http_adapter::{HttpAdapter, HttpClient, NetworkPolicy};
use poe_wayfinder_app::adapter::trade_api_adapter::TradeUrls;
use poe_wayfinder_app::config::PoeWayfinderDatagenConfig;
use poe_wayfinder_app::controller::datagen_controller::{
    augment_to_ndjson, build_augments, build_items, build_stats, build_trade_tags, item_to_ndjson,
    stat_to_ndjson,
};
use poe_wayfinder_app::logging::{Logger, Value};
use poe_wayfinder_core::types::GameVersion;

fn main() -> ExitCode {
    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(err) => {
            eprintln!("poe-wayfinder-datagen: starting the runtime: {err}");

            return ExitCode::FAILURE;
        }
    };

    if let Some(dir) = augments_only() {
        let log = Logger::new("info", "poe-wayfinder-datagen");

        write_augments(Path::new(&dir), GameVersion::Poe2, &log);

        return ExitCode::SUCCESS;
    }

    runtime.block_on(run())
}

fn augments_only() -> Option<String> {
    let mut args = std::env::args().skip(1);

    while let Some(arg) = args.next() {
        if arg == "--augments-only" {
            return args.next();
        }
    }

    None
}

async fn run() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();

    let cfg = match PoeWayfinderDatagenConfig::load(&args) {
        Ok(cfg) => cfg,
        Err(err) => {
            eprintln!("poe-wayfinder-datagen: loading config: {err}");

            return ExitCode::FAILURE;
        }
    };

    let log = Logger::new(&cfg.log_level, "poe-wayfinder-datagen");

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
                ("error", Value::Str(poe_wayfinder_app::util::error_chain::render(&err))),
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
                &[("error", Value::Str(poe_wayfinder_app::util::error_chain::render(&err)))],
            );

            return ExitCode::FAILURE;
        }
    };

    let urls = TradeUrls::new(&cfg.trade_base_url, game);

    let stats_body = match fetch(&http, &urls.data("stats"), &log).await {
        Some(body) => body,
        None => return ExitCode::FAILURE,
    };

    let items_body = match fetch(&http, &urls.data("items"), &log).await {
        Some(body) => body,
        None => return ExitCode::FAILURE,
    };

    let static_body = match fetch(&http, &urls.data("static"), &log).await {
        Some(body) => body,
        None => return ExitCode::FAILURE,
    };

    let trade_tags = match build_trade_tags(&static_body) {
        Ok(tags) => tags,
        Err(err) => {
            log.error(
                "building trade tags",
                &[("error", Value::Str(poe_wayfinder_app::util::error_chain::render(&err)))],
            );

            return ExitCode::FAILURE;
        }
    };

    let stats = match build_stats(&stats_body) {
        Ok(stats) => stats,
        Err(err) => {
            log.error("building stats", &[("error", Value::Str(poe_wayfinder_app::util::error_chain::render(&err)))]);

            return ExitCode::FAILURE;
        }
    };

    let items = match build_items(&items_body, &trade_tags) {
        Ok(items) => items,
        Err(err) => {
            log.error("building items", &[("error", Value::Str(poe_wayfinder_app::util::error_chain::render(&err)))]);

            return ExitCode::FAILURE;
        }
    };

    let out_dir = Path::new(&cfg.out_dir);

    if let Err(err) = std::fs::create_dir_all(out_dir) {
        log.error(
            "creating the output directory",
            &[
                ("out_dir", Value::Str(cfg.out_dir.clone())),
                ("error", Value::Str(poe_wayfinder_app::util::error_chain::render(&err))),
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
        let body = format!("{}\n", lines.join("\n"));

        if let Err(err) = std::fs::write(&path, body) {
            log.error(
                "writing the data file",
                &[
                    ("path", Value::Str(path.display().to_string())),
                    ("error", Value::Str(poe_wayfinder_app::util::error_chain::render(&err))),
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

    write_augments(out_dir, game, &log);

    let with_category = items.iter().filter(|i| i.category.is_some()).count();

    log.info(
        "category coverage",
        &[
            ("with_category", Value::Int(with_category as i64)),
            ("total", Value::Int(items.len() as i64)),
        ],
    );

    log.warn(
        "weapon and armour categories, roll ranges and modifier tiers are not built here. They come from the game bundles and are vendored in poe-wayfinder-data/tables.",
        &[],
    );

    ExitCode::SUCCESS
}

const AUGMENT_SOURCES: &[&str] = &[
    "../reference/Exiled-Exchange-2/renderer/public/data/en/items.ndjson",
    "reference/Exiled-Exchange-2/renderer/public/data/en/items.ndjson",
];

fn write_augments(out_dir: &Path, game: GameVersion, log: &Logger) {
    if game != GameVersion::Poe2 {
        log.info(
            "runes and soul cores are Path of Exile 2 only, so no augment file is written",
            &[],
        );

        return;
    }

    let Some(source) = AUGMENT_SOURCES.iter().map(Path::new).find(|p| p.exists()) else {
        log.warn(
            "no augment source found, so the item editor will have nothing to offer. Check out the reference beside this workspace and run datagen again.",
            &[],
        );

        return;
    };

    let body = match std::fs::read_to_string(source) {
        Ok(body) => body,
        Err(err) => {
            log.error(
                "reading the augment source",
                &[
                    ("path", Value::Str(source.display().to_string())),
                    ("error", Value::Str(poe_wayfinder_app::util::error_chain::render(&err))),
                ],
            );

            return;
        }
    };

    let records = build_augments(&body);
    let lines: Vec<String> = records.iter().map(augment_to_ndjson).collect();
    let path = out_dir.join("augments.ndjson");

    if let Err(err) = std::fs::write(&path, format!("{}\n", lines.join("\n"))) {
        log.error(
            "writing the augment file",
            &[
                ("path", Value::Str(path.display().to_string())),
                ("error", Value::Str(poe_wayfinder_app::util::error_chain::render(&err))),
            ],
        );

        return;
    }

    log.info(
        "wrote data file",
        &[
            ("path", Value::Str(path.display().to_string())),
            ("records", Value::Int(records.len() as i64)),
        ],
    );
}

async fn fetch(http: &HttpAdapter, url: &str, log: &Logger) -> Option<String> {
    let response = match http.get(url, &[("accept", "application/json")]).await {
        Ok(response) => response,
        Err(err) => {
            log.error(
                "fetching a data table",
                &[
                    ("url", Value::Str(url.to_string())),
                    ("error", Value::Str(poe_wayfinder_app::util::error_chain::render(&err))),
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
