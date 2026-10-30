//! `allowit skills search QUERY`: searches catalog services.
use super::ROUTE;
use crate::{
    error::Result,
    gateway::{args::Invocation, control},
};

pub(crate) fn run(inv: &Invocation, query: Option<&String>, stdout: &mut String) -> Result<i32> {
    control::read(inv, ROUTE, query, stdout)
}
