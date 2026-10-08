//! Replace only policy.rs. This host template compiles authored Rust and evaluates checked IR.
#![allow(dead_code, unused_variables)]
include!("policy.rs");

/// Authenticated adapters supply Context. Mandatory allocation checks run in the evaluator.
pub fn execute(ctx: &Context) -> Result<allowit::Decision, allowit::CompileError> {
    let policy = allowit::compile(include_str!("policy.rs"))?;
    Ok(allowit::evaluate(&policy, allowit::Profile::Contract, ctx))
}

#[cfg(test)]
mod tests {
    #[test]
    fn complete_policy_is_valid_rust_and_checked_policy_source() {
        let config = super::new();
        let _ = config;
        let _ = super::_execute;
        allowit::compile(include_str!("policy.rs")).unwrap();
    }
}
