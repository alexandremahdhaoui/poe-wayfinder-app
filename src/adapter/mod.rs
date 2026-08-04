//! The outside world.
//!
//! `http_adapter` is the only module in the project that builds an HTTP
//! client. Everything that needs the network takes it as a dependency, so the
//! allowlist has exactly one place to live.

pub mod game_data_adapter;
pub mod http_adapter;
pub mod rate_limit_adapter;
pub mod trade_api_adapter;
