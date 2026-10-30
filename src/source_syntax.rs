//! Intrinsic Rust error syntax. This does not register a replacement system function.
use crate::CompileError;
use proc_macro2::{Delimiter, Group, Literal, Punct, Spacing, TokenStream, TokenTree};
fn invalid() -> CompileError {
    CompileError::new(
        "INVALID_POLICY_ERROR",
        "Use Err(PolicyError { code: \"CODE\".into(), reason: \"Reason\".into() }) with bounded literal strings.",
    )
}
pub(crate) fn errors(tokens: TokenStream) -> Result<TokenStream, CompileError> {
    let mut input = tokens.into_iter().peekable();
    let mut output = TokenStream::new();
    while let Some(token) = input.next() {
        if matches!(&token,TokenTree::Ident(name)if name=="Err") {
            let Some(TokenTree::Group(group)) = input.next() else {
                return Err(invalid());
            };
            if group.delimiter() != Delimiter::Parenthesis {
                return Err(invalid());
            }
            let value = syn::parse2::<syn::ExprStruct>(group.stream()).map_err(|_| invalid())?;
            if !value.path.is_ident("PolicyError")
                || !value.attrs.is_empty()
                || value.rest.is_some()
                || value.fields.len() != 2
            {
                return Err(invalid());
            }
            let mut code = None;
            let mut reason = None;
            for field in value.fields {
                let syn::Member::Named(name) = field.member else {
                    return Err(invalid());
                };
                if !field.attrs.is_empty() || field.colon_token.is_none() {
                    return Err(invalid());
                }
                let syn::Expr::MethodCall(call) = field.expr else {
                    return Err(invalid());
                };
                if call.method != "into"
                    || !call.args.is_empty()
                    || !call.attrs.is_empty()
                    || call.turbofish.is_some()
                {
                    return Err(invalid());
                }
                let syn::Expr::Lit(literal) = *call.receiver else {
                    return Err(invalid());
                };
                let syn::Lit::Str(value) = literal.lit else {
                    return Err(invalid());
                };
                if !literal.attrs.is_empty() || !value.suffix().is_empty() {
                    return Err(invalid());
                }
                let destination = if name == "code" {
                    &mut code
                } else if name == "reason" {
                    &mut reason
                } else {
                    return Err(invalid());
                };
                if destination.is_some() {
                    return Err(invalid());
                }
                *destination = Some(value);
            }
            let (code, reason) = (code.ok_or_else(invalid)?, reason.ok_or_else(invalid)?);
            if code.value().is_empty()
                || code.value().len() > 128
                || reason.value().is_empty()
                || reason.value().len() > 4096
            {
                return Err(invalid());
            }
            let mut arguments = TokenStream::new();
            for (index, value) in [code, reason].into_iter().enumerate() {
                if index > 0 {
                    arguments.extend([TokenTree::Punct(Punct::new(',', Spacing::Alone))]);
                }
                let mut literal = Literal::string(&value.value());
                literal.set_span(value.span());
                arguments.extend([TokenTree::Literal(literal)]);
            }
            let mut rewritten = Group::new(Delimiter::Parenthesis, arguments);
            rewritten.set_span(group.span());
            output.extend([token, TokenTree::Group(rewritten)]);
        } else if let TokenTree::Group(group) = token {
            let mut rewritten = Group::new(group.delimiter(), errors(group.stream())?);
            rewritten.set_span(group.span());
            output.extend([TokenTree::Group(rewritten)]);
        } else {
            output.extend([token]);
        }
    }
    Ok(output)
}
