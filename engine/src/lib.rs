//! Allowit backend requests and service composition, with no terminal I/O.
//! Callers pass arguments and readers in and receive values or `String` output.
pub use allowit_sdk::client::error;
pub mod client;
pub mod config;
pub mod gateway;
pub mod output;
pub mod request;
pub mod routing;
#[cfg(feature = "services")]
pub mod services;
pub mod skill;
pub mod typed_request;
pub mod wallet;
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
