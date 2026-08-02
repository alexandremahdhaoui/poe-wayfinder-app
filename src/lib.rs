//! Everything that touches the operating system or the network.
//!
//! `poe-trader-core` holds the domain and opens no socket. This crate supplies
//! the traits that crate declares, plus the window, the clipboard, the input
//! hooks and the drivers that start them.

pub mod adapter;
pub mod config;
pub mod controller;
pub mod driver;
pub mod logging;
pub mod resilience;
pub mod telemetry;
pub mod types;
