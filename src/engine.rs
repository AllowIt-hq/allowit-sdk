//! Source type-checking facade. Generated native checks call the shared onchain engine.
use crate::{Context, Spent, State, prelude::PolicyError};
fn required() -> Result<Spent, PolicyError> {
    Err(PolicyError {
        code: "NATIVE_ENGINE_REQUIRED".into(),
        reason: "Use the generated native policy and shared onchain engine.".into(),
    })
}
macro_rules! check {($($name:ident),*)=>{$(
    /// Select this shared engine validation with the complete trusted engine state.
    pub fn $name(_request:&Context,_state:&State)->Result<Spent,PolicyError>{required()}
)*};}
check!(
    daily_cap,
    transaction_cap,
    total_cap,
    call_cap,
    recipient,
    expiry,
    token
);
