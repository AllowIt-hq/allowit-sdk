//! Full-source owner approvals are independent of the unchanged native rule prefix.
use crate::CompileError;
use alloc::{format, string::String, vec::Vec};
use serde::{Deserialize, Serialize};

pub const SOURCE_UPDATE_DOMAIN: &str = "AllowIt offchain policy source v1";

/// Exact UTF-8 source bytes and the immutable native binding approved by the owner.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SourceUpdate {
    pub owner: String,
    pub network: String,
    pub wallet: String,
    pub policy_instance: String,
    pub prefix_digest: String,
    pub source_hash: String,
    pub revision: u64,
    pub source: String,
}

/// Supplied from the current native binding and the durable last accepted source revision.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceUpdateBinding {
    pub owner: String,
    pub network: String,
    pub wallet: String,
    pub policy_instance: String,
    pub prefix_digest: String,
    pub current_revision: u64,
    pub current_source_hash: String,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SourceUpdateDisposition {
    Advance,
    Idempotent,
}

const STANDARD_RULES: &[&str] = &[
    "spent_cap",
    "day_spent_cap",
    "day_total_cap",
    "transaction_cap",
    "call_cap",
    "recipients",
    "expiry",
    "tokens",
];
/// The compiler calls this only after it validates the complete source.
/// Only checked top-level calls at the beginning belong to the standard native prefix.
pub(crate) fn prefix_from_function(function: &syn::ItemFn) -> Vec<String> {
    fn path(expr: &syn::Expr) -> Option<Vec<String>> {
        let syn::Expr::Path(path) = expr else {
            return None;
        };
        if path.qself.is_some()
            || path.path.leading_colon.is_some()
            || path
                .path
                .segments
                .iter()
                .any(|segment| !matches!(segment.arguments, syn::PathArguments::None))
        {
            return None;
        }
        Some(
            path.path
                .segments
                .iter()
                .map(|segment| segment.ident.to_string())
                .collect(),
        )
    }
    fn rule(statement: &syn::Stmt) -> Option<String> {
        let syn::Stmt::Expr(syn::Expr::Try(checked), Some(_)) = statement else {
            return None;
        };
        let syn::Expr::Call(call) = checked.expr.as_ref() else {
            return None;
        };
        let function = path(&call.func)?;
        if function.len() != 2
            || function[0] != "allowit"
            || !STANDARD_RULES.contains(&function[1].as_str())
            || call.args.len() != 1
        {
            return None;
        }
        let reference = path(&call.args[0])?;
        if reference.len() != 2 || reference[0] != "grant" || reference[1] != function[1] {
            return None;
        }
        Some(function[1].clone())
    }
    function.block.stmts.iter().map_while(rule).collect()
}
pub fn initial_rule_prefix(source: &str) -> Result<Vec<String>, CompileError> {
    crate::compile(source)?;
    let file =
        syn::parse_file(source).map_err(|_| invalid("The policy source cannot be parsed."))?;
    let function = file
        .items
        .iter()
        .find_map(|item| match item {
            syn::Item::Fn(function) if function.sig.ident == "_execute" => Some(function),
            _ => None,
        })
        .ok_or_else(|| invalid("The policy must declare _execute."))?;
    Ok(prefix_from_function(function))
}

fn invalid(message: &str) -> CompileError {
    CompileError::new("INVALID_SOURCE_UPDATE", message)
}
fn digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        && value.bytes().any(|byte| byte != b'0')
}
impl SourceUpdate {
    pub fn validate_fields(&self) -> Result<(), CompileError> {
        if self.owner.is_empty()
            || self.owner.len() > 128
            || self.wallet.is_empty()
            || self.wallet.len() > 128
            || !self.network.starts_with("stellar:")
            || self.network.len() > 64
            || self.owner.chars().any(char::is_control)
            || self.wallet.chars().any(char::is_control)
            || !digest(&self.policy_instance)
            || !digest(&self.prefix_digest)
            || !digest(&self.source_hash)
            || self.revision == 0
            || self.revision > 9_007_199_254_740_991
            || self.source.len() > crate::MAX_SOURCE_BYTES
            || crate::digest(self.source.as_bytes()) != self.source_hash
        {
            return Err(invalid(
                "Bind the exact source bytes, native identities and bounded next revision.",
            ));
        }
        Ok(())
    }
    /// Fixed field order is part of the signing protocol. No source normalization occurs.
    pub fn signing_message(&self) -> Result<String, CompileError> {
        self.validate_fields()?;
        let json = serde_json::to_string(self)
            .map_err(|_| invalid("The source update cannot be encoded."))?;
        Ok(format!("{SOURCE_UPDATE_DOMAIN}\n{json}"))
    }
    /// Validate identity and optimistic revision. The native adapter also compiles and
    /// checks the actual new-source prefix before verifying the current owner's quorum.
    pub fn validate_binding(
        &self,
        binding: &SourceUpdateBinding,
    ) -> Result<SourceUpdateDisposition, CompileError> {
        self.validate_fields()?;
        if self.owner != binding.owner
            || self.network != binding.network
            || self.wallet != binding.wallet
            || self.policy_instance != binding.policy_instance
            || self.prefix_digest != binding.prefix_digest
        {
            return Err(invalid(
                "The update differs from the current native policy binding.",
            ));
        }
        if self.revision == binding.current_revision
            && self.source_hash == binding.current_source_hash
        {
            return Ok(SourceUpdateDisposition::Idempotent);
        }
        if binding.current_revision.checked_add(1) != Some(self.revision) {
            return Err(invalid("Sign the exact next offchain source revision."));
        }
        Ok(SourceUpdateDisposition::Advance)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn update() -> SourceUpdate {
        let source = "async fn _execute(ctx: &Context) -> PolicyResult { Ok(()) }".to_string();
        SourceUpdate {
            owner: "GOWNER".into(),
            network: "stellar:testnet".into(),
            wallet: "CWALLET".into(),
            policy_instance: "01".repeat(32),
            prefix_digest: "02".repeat(32),
            source_hash: crate::digest(source.as_bytes()),
            revision: 2,
            source,
        }
    }
    #[test]
    fn exact_source_and_native_binding_are_part_of_the_message() {
        let update = update();
        let message = update.signing_message().unwrap();
        let mut changed = update.clone();
        changed.source.push('\n');
        assert!(changed.signing_message().is_err());
        changed.source_hash = crate::digest(changed.source.as_bytes());
        assert_ne!(message, changed.signing_message().unwrap());
        let mut value = serde_json::to_value(&update).unwrap();
        value["grant"] = serde_json::json!({});
        assert!(serde_json::from_value::<SourceUpdate>(value).is_err());
    }
    #[test]
    fn source_revision_is_an_atomic_compare_and_swap_not_a_replay_counter_reset() {
        let update = update();
        let mut binding = SourceUpdateBinding {
            owner: update.owner.clone(),
            network: update.network.clone(),
            wallet: update.wallet.clone(),
            policy_instance: update.policy_instance.clone(),
            prefix_digest: update.prefix_digest.clone(),
            current_revision: 1,
            current_source_hash: "03".repeat(32),
        };
        assert_eq!(
            update.validate_binding(&binding).unwrap(),
            SourceUpdateDisposition::Advance
        );
        binding.current_revision = 2;
        binding.current_source_hash = update.source_hash.clone();
        assert_eq!(
            update.validate_binding(&binding).unwrap(),
            SourceUpdateDisposition::Idempotent
        );
        binding.current_revision = 3;
        assert!(update.validate_binding(&binding).is_err());
        binding.current_revision = 1;
        binding.prefix_digest = "04".repeat(32);
        assert!(update.validate_binding(&binding).is_err());
    }
    #[test]
    fn native_prefix_stops_at_first_other_statement_and_preserves_order() {
        let source = r#"async fn _execute(ctx:&Context)->PolicyResult {
            allowit::recipients(grant::recipients)?;
            allowit::spent_cap(grant::spent_cap)?;
            allowit::spent_cap(grant::spent_cap)?;
            let amount=ctx.amount_units;
            if amount>0 {allowit::day_total_cap(grant::day_total_cap)?;}
            allowit::tokens(grant::tokens)?;
            Ok(())
        }"#;
        // Parse directly so this structural test does not need execution context.
        let function: syn::ItemFn = syn::parse_str(source).unwrap();
        assert_eq!(
            prefix_from_function(&function),
            ["recipients", "spent_cap", "spent_cap"]
        );
        let conditional:syn::ItemFn=syn::parse_str("async fn _execute(ctx:&Context)->PolicyResult { if true {allowit::spent_cap(grant::spent_cap)?;} Ok(()) }").unwrap();
        assert!(prefix_from_function(&conditional).is_empty());
    }
}
