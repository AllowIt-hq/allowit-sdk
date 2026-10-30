//! Explicit server checks share the owner-approved source with the onchain handler.
//! The native compiler never treats these checks as contract-enforced predicates.
use crate::{CompileError, CompiledPolicy};
use proc_macro2::TokenStream;
use syn::{Item, spanned::Spanned};

#[derive(Clone, Debug)]
pub struct ServerPolicy {
    /// The complete source identity approved by the owner, including both handlers.
    pub owner_source_hash: String,
    /// Independently valid executable artifact for the explicitly server-only handler.
    pub policy: CompiledPolicy,
}

fn invalid() -> CompileError {
    CompileError::new(
        "INVALID_SERVER_POLICY",
        "Declare at most one private async _server_execute(ctx: &Context) -> PolicyResult handler.",
    )
}
fn mask(source: &str, ranges: &[core::ops::Range<usize>]) -> String {
    let mut bytes = source.as_bytes().to_vec();
    for range in ranges {
        for byte in &mut bytes[range.clone()] {
            if !matches!(*byte, b'\n' | b'\r') {
                *byte = b' ';
            }
        }
    }
    String::from_utf8(bytes).expect("Masked complete Rust items preserve UTF-8")
}
fn hook_source(source: &str, file: &syn::File) -> Result<Option<String>, CompileError> {
    let hooks: Vec<_> = file
        .items
        .iter()
        .filter_map(|item| match item {
            Item::Fn(function) if function.sig.ident == "_server_execute" => Some(function),
            _ => None,
        })
        .collect();
    if hooks.is_empty() {
        return Ok(None);
    }
    if hooks.len() != 1 {
        return Err(invalid());
    }
    let hook = hooks[0];
    if !matches!(hook.vis, syn::Visibility::Inherited)
        || !hook.attrs.is_empty()
        || hook.sig.inputs.len() != 1
    {
        return Err(invalid());
    }
    let ranges: Vec<_> = file
        .items
        .iter()
        .filter_map(|item| {
            if matches!(item, Item::Use(_)) || item.span().byte_range() == hook.span().byte_range()
            {
                None
            } else {
                Some(item.span().byte_range())
            }
        })
        .collect();
    let mut server = mask(source, &ranges).into_bytes();
    let name = hook.sig.ident.span().byte_range();
    if source.get(name.clone()) != Some("_server_execute") {
        return Err(invalid());
    }
    server[name.clone()].fill(b' ');
    server[name.start..name.start + "_execute".len()].copy_from_slice(b"_execute");
    Ok(Some(String::from_utf8(server).map_err(|_| invalid())?))
}

/// Called only after the normal compiler checks source size and token depth.
pub(crate) fn strip_server_hook(
    source: &str,
    tokens: TokenStream,
) -> Result<TokenStream, CompileError> {
    // Avoid changing parsing and diagnostic behavior for every existing policy.
    if !tokens.clone().into_iter().any(|token| matches!(token, proc_macro2::TokenTree::Ident(ref ident) if ident == "_server_execute")) {
        return Ok(tokens);
    }
    let file = syn::parse2::<syn::File>(tokens.clone()).map_err(|_| invalid())?;
    let Some(server) = hook_source(source, &file)? else {
        return Ok(tokens);
    };
    // Keep the outer compiler's spans alive until its own cleanup guard exits.
    let compiled = crate::compiler::compile_inner(&server)?;
    if compiled.calls.iter().any(|call| {
        !["context_u64", "fail", "is_one_of"].contains(&call.name.rsplit("::").next().unwrap_or(""))
    }) {
        return Err(CompileError::new(
            "INVALID_SERVER_POLICY",
            "Server accounting checks use context_u64, is_one_of, fail and integer control flow. Keep native spending checks in _execute.",
        ));
    }
    let hook = file
        .items
        .iter()
        .find(|item| matches!(item, Item::Fn(function) if function.sig.ident == "_server_execute"))
        .ok_or_else(invalid)?;
    // Parse the masked source again. Kept byte lengths preserve source locations.
    mask(source, &[hook.span().byte_range()])
        .parse()
        .map_err(|_| invalid())
}

/// Returns the separately validated server handler. None preserves existing behavior.
/// The returned source identity binds the complete owner-authored policy, including both handlers.
pub fn compile_stellar_server_policy(source: &str) -> Result<Option<ServerPolicy>, CompileError> {
    let onchain = crate::compile(source)?;
    let tokens = source.parse::<TokenStream>().map_err(|_| invalid())?;
    let file = syn::parse2::<syn::File>(tokens).map_err(|_| invalid())?;
    let Some(server) = hook_source(source, &file)? else {
        return Ok(None);
    };
    let compiled = crate::compile(&server)?;
    Ok(Some(ServerPolicy {
        owner_source_hash: onchain.source_hash,
        policy: compiled,
    }))
}

/// Owner-reviewable onchain source. The full source hash still binds the server hook.
pub fn split_stellar_source(source: &str) -> Result<String, CompileError> {
    crate::compile(source)?;
    let tokens = source.parse::<TokenStream>().map_err(|_| invalid())?;
    let file = syn::parse2::<syn::File>(tokens).map_err(|_| invalid())?;
    let ranges: Vec<_> = file
        .items
        .iter()
        .filter_map(|item| match item {
            Item::Fn(function) if function.sig.ident == "_server_execute" => {
                Some(item.span().byte_range())
            }
            _ => None,
        })
        .collect();
    Ok(mask(source, &ranges))
}

/// Evaluate the explicit server hook with the adapter's exact asset units.
/// The caller must bind the complete approved source and replace all request context with trusted facts.
pub fn evaluate_stellar_server_policy(
    policy: &ServerPolicy,
    context: &crate::Context,
) -> crate::Decision {
    crate::runtime::evaluate_server_accounting(&policy.policy, context)
}
