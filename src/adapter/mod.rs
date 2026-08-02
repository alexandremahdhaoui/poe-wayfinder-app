//! The outside world.
//!
//! `http_adapter` is the only module in the project that builds an HTTP
//! client. Everything that needs the network takes it as a dependency, so the
//! allowlist has exactly one place to live.

pub mod http_adapter;
