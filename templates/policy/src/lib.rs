//! Replace policy.rs. The template checks authored Rust and the restricted compiler.
#![allow(dead_code, unused_variables)]
include!("policy.rs");

/// Trusted adapters supply the native operation, owner Grant and authoritative Spent.
pub fn execute(ctx: &Context) -> Result<allowit::Decision, allowit::CompileError> {
    let policy = allowit::compile(include_str!("policy.rs"))?;
    Ok(allowit::evaluate_with_policy_rules(&policy, ctx))
}

#[cfg(test)]
mod tests {
    #[test]
    fn policy_is_valid_rust_and_checked_source() {
        let _ = super::_execute;
        allowit::compile(include_str!("policy.rs")).unwrap();
    }
}
