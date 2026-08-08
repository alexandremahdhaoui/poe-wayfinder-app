use crate::adapter::game_log_adapter::{GameLogWatcher, NoLog};
use crate::adapter::http_adapter::{HttpAdapter, NetworkPolicy};
use crate::config::PoeTraderConfig;
use crate::controller::log_watch_controller::{LogSource, LogWatchController};
use crate::logging::{Logger, Value};

use std::time::Duration;

pub fn build_http(cfg: &PoeTraderConfig, log: &Logger) -> Option<HttpAdapter> {
    let policy = NetworkPolicy::new(
        cfg.network_enabled,
        cfg.block_unlisted_hosts,
        &cfg.allowed_hosts,
    );

    log.info(
        "network policy",
        &[
            ("enabled", Value::Bool(cfg.network_enabled)),
            ("block_unlisted", Value::Bool(cfg.block_unlisted_hosts)),
            ("hosts", Value::Str(policy.allowed_hosts().join(","))),
        ],
    );

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

            return None;
        }
    };

    Some(http)
}

pub fn build_logs(client_log_path: &str) -> Box<dyn LogSource> {
    match client_log_path.is_empty() {
        true => Box::new(LogWatchController::new(NoLog)),
        false => Box::new(LogWatchController::new(GameLogWatcher::new(
            std::path::Path::new(client_log_path),
        ))),
    }
}

#[cfg(windows)]
pub fn build_copier(
    restore: bool,
    log: &Logger,
) -> Option<
    crate::controller::copy_controller::CopyController<
        crate::adapter::clipboard_adapter::SystemClipboard,
        crate::adapter::game_window_adapter::KeyboardCopyTrigger,
    >,
> {
    use crate::adapter::clipboard_adapter::{CopyTiming, SystemClipboard};
    use crate::adapter::game_window_adapter::KeyboardCopyTrigger;
    use crate::controller::copy_controller::CopyController;

    let clipboard = match SystemClipboard::new() {
        Ok(clipboard) => clipboard,
        Err(err) => {
            log.error(
                "opening the clipboard",
                &[("error", Value::Str(err.to_string()))],
            );

            return None;
        }
    };

    Some(CopyController::new(
        clipboard,
        KeyboardCopyTrigger::new(),
        CopyTiming::default(),
        restore,
    ))
}
