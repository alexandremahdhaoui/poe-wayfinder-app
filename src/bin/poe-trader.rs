//! The overlay.
//!
//! main reads config, builds adapters, injects them into controllers, injects
//! controllers into drivers, then starts the drivers. It calls nothing else.
//!
//! Controllers and drivers are not written yet. What runs today is the part
//! that must be right before any of them exist: config resolution and the
//! network policy.

use std::process::ExitCode;
use std::time::Duration;

use poe_trader_app::adapter::http_adapter::{HttpAdapter, NetworkPolicy, PolicyError};
use poe_trader_app::config::PoeTraderConfig;
use poe_trader_app::logging::{Logger, Value};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();

    let cfg = match PoeTraderConfig::load(&args) {
        Ok(cfg) => cfg,
        Err(err) => {
            eprintln!("poe-trader: loading config: {err}");

            return ExitCode::FAILURE;
        }
    };

    let log = Logger::new(&cfg.log_level, "poe-trader");

    let policy = NetworkPolicy::new(
        cfg.network_enabled,
        cfg.block_unlisted_hosts,
        &cfg.allowed_hosts,
    );

    // Log the policy at startup. An operator must never have to read source to
    // answer "what can this thing reach".
    log.info(
        "network policy",
        &[
            ("enabled", Value::Bool(cfg.network_enabled)),
            ("block_unlisted", Value::Bool(cfg.block_unlisted_hosts)),
            ("hosts", Value::Str(policy.allowed_hosts().join(","))),
        ],
    );

    // poesessid is never logged. Only whether one was supplied.
    log.info(
        "session",
        &[("poesessid_present", Value::Bool(!cfg.poesessid.is_empty()))],
    );

    let http = match HttpAdapter::new(policy, Duration::from_secs(30)) {
        Ok(http) => http,
        Err(err) => {
            log.error(
                "building http client",
                &[("error", Value::Str(err.to_string()))],
            );

            return ExitCode::FAILURE;
        }
    };

    // Prove the policy is live before anything else starts. A trade_base_url
    // the allowlist refuses is a config mistake, and failing here beats
    // failing on the first hotkey press.
    //
    // NetworkDisabled is not a mistake. It is the documented way to run the
    // parser offline, so it warns and keeps going.
    match http.policy().check(&cfg.trade_base_url) {
        Ok(()) => {}
        Err(PolicyError::NetworkDisabled) => {
            log.warn("network is disabled. Pricing will not work.", &[]);
        }
        Err(err) => {
            log.error(
                "trade_base_url is refused by the network policy",
                &[
                    ("url", Value::Str(cfg.trade_base_url.clone())),
                    ("error", Value::Str(err.to_string())),
                ],
            );

            return ExitCode::FAILURE;
        }
    }

    log.info(
        "startup",
        &[
            ("game", Value::Str(cfg.game.clone())),
            ("window_title", Value::Str(cfg.window_title.clone())),
            ("data_dir", Value::Str(cfg.data_dir.clone())),
        ],
    );

    log.warn("drivers are not implemented yet", &[]);

    ExitCode::SUCCESS
}
