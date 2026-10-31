//! `allowit skills show SERVICE`: shows one service and its endpoints through
//! `GET /api/gateway/services?query=SERVICE`.
//!
//! ```text
//! allowit skills search weather
//! allowit skills show SERVICE_ID --json
//! allowit call SERVICE_ID --input '{"city":"Paris"}'
//! ```
use super::ROUTE;
use crate::{
    error::Result,
    gateway::{args::Invocation, control},
};

pub(crate) fn run(inv: &Invocation, query: Option<&String>, stdout: &mut String) -> Result<i32> {
    control::read(inv, ROUTE, query, stdout)
}
