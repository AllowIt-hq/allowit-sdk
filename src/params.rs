//! Bounded declaration parser for immutable constructor parameters.
//! This runs before syn, so recursive types and initializer expressions never reach it.
use crate::{CompileError, Expr};
use proc_macro2::{Delimiter, TokenStream, TokenTree};
use std::collections::BTreeMap;

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Number,
    Boolean,
    String,
    Strings,
    OwnerLimit,
}
fn invalid() -> CompileError {
    CompileError::new(
        "INVALID_POLICY",
        "Declare PolicyParams with primitive fields and initialize every field with a literal in fn new() -> PolicyParams.",
    )
}
fn ident(token: &TokenTree, name: &str) -> bool {
    matches!(token, TokenTree::Ident(i) if i == name)
}
fn punctuation(token: &TokenTree, c: char) -> bool {
    matches!(token, TokenTree::Punct(p) if p.as_char() == c)
}
fn exact(tokens: &[TokenTree], words: &[&str]) -> bool {
    tokens.len() == words.len() && tokens.iter().zip(words).all(|(t, w)| t.to_string() == *w)
}
fn fields(tokens: TokenStream) -> Result<Vec<(String, Vec<TokenTree>)>, CompileError> {
    let tokens: Vec<_> = tokens.into_iter().collect();
    let mut result = vec![];
    for (index, field) in tokens.split(|t| punctuation(t, ',')).enumerate() {
        if field.is_empty() {
            if index == 0 && !tokens.is_empty()
                || index + 1 != tokens.split(|t| punctuation(t, ',')).count()
            {
                return Err(invalid());
            }
            continue;
        }
        let [TokenTree::Ident(name), colon, rest @ ..] = field else {
            return Err(invalid());
        };
        if !punctuation(colon, ':')
            || rest.is_empty()
            || result.iter().any(|(n, _)| n == &name.to_string())
        {
            return Err(invalid());
        }
        syn::parse_str::<syn::Ident>(&name.to_string()).map_err(|_| invalid())?;
        if name.to_string().starts_with("r#") {
            return Err(invalid());
        }
        result.push((name.to_string(), rest.to_vec()));
    }
    if result.len() > 32 {
        return Err(invalid());
    }
    Ok(result)
}
fn kind(tokens: &[TokenTree]) -> Result<Kind, CompileError> {
    if exact(tokens, &["OwnerLimit"]) {
        return Ok(Kind::OwnerLimit);
    }
    if exact(tokens, &["u64"]) {
        return Ok(Kind::Number);
    }
    if exact(tokens, &["bool"]) {
        return Ok(Kind::Boolean);
    }
    if exact(tokens, &["&", "'", "static", "str"]) {
        return Ok(Kind::String);
    }
    if let [a, b, c, TokenTree::Group(g)] = tokens
        && exact(&[a.clone(), b.clone(), c.clone()], &["&", "'", "static"])
        && g.delimiter() == Delimiter::Bracket
        && exact(
            &g.stream().into_iter().collect::<Vec<_>>(),
            &["&", "'", "static", "str"],
        )
    {
        return Ok(Kind::Strings);
    }
    Err(invalid())
}
fn literal(tokens: &[TokenTree], kind: Kind) -> Result<Expr, CompileError> {
    match (kind, tokens) {
        (Kind::OwnerLimit, [a, c1, c2, f, TokenTree::Group(args)])
            if exact(
                &[a.clone(), c1.clone(), c2.clone(), f.clone()],
                &["allowit", ":", ":", "owner_limit"],
            ) && matches!(c1,TokenTree::Punct(p) if p.spacing()==proc_macro2::Spacing::Joint)
                && args.delimiter() == Delimiter::Parenthesis =>
        {
            let args: Vec<_> = args.stream().into_iter().collect();
            let [key, comma, amount] = args.as_slice() else {
                return Err(invalid());
            };
            if !punctuation(comma, ',') {
                return Err(invalid());
            }
            let key = literal(core::slice::from_ref(key), Kind::String)?;
            let amount = literal(core::slice::from_ref(amount), Kind::Number)?;
            if !matches!(&key,Expr::String{value} if value=="native_daily_limit" || value=="native_action_limit")
                || !matches!(
                    amount,
                    Expr::Integer {
                        value: 1..=50_000_000
                    }
                )
            {
                return Err(invalid());
            }
            Ok(Expr::Array {
                values: vec![key, amount],
            })
        }
        (Kind::Number, [TokenTree::Literal(l)]) => {
            let v = syn::parse_str::<syn::LitInt>(&l.to_string()).map_err(|_| invalid())?;
            if !v.suffix().is_empty() {
                return Err(invalid());
            }
            Ok(Expr::Integer {
                value: v.base10_parse().map_err(|_| invalid())?,
            })
        }
        (Kind::String, [TokenTree::Literal(l)]) => {
            let v = syn::parse_str::<syn::LitStr>(&l.to_string()).map_err(|_| invalid())?;
            if !v.suffix().is_empty() || v.value().len() > 1024 {
                return Err(invalid());
            }
            Ok(Expr::String { value: v.value() })
        }
        (Kind::Boolean, [t]) if ident(t, "true") || ident(t, "false") => Ok(Expr::Boolean {
            value: ident(t, "true"),
        }),
        (Kind::Strings, [amp, TokenTree::Group(g)])
            if punctuation(amp, '&') && g.delimiter() == Delimiter::Bracket =>
        {
            let entries: Vec<_> = g.stream().into_iter().collect();
            let mut values = vec![];
            for (index, entry) in entries.split(|t| punctuation(t, ',')).enumerate() {
                if entry.is_empty() {
                    if index == 0 || index + 1 != entries.split(|t| punctuation(t, ',')).count() {
                        return Err(invalid());
                    }
                } else {
                    values.push(literal(entry, Kind::String)?);
                }
            }
            if values.is_empty() || values.len() > 32 {
                return Err(invalid());
            }
            Ok(Expr::Array { values })
        }
        _ => Err(invalid()),
    }
}

/// Remove the exact optional parameter declarations, retaining original entrypoint spans.
pub(crate) fn extract(
    tokens: &TokenStream,
) -> Result<(TokenStream, Option<BTreeMap<String, Expr>>), CompileError> {
    let tokens: Vec<_> = tokens.clone().into_iter().collect();
    let start = if tokens.first().is_some_and(|t| ident(t, "use")) {
        tokens
            .iter()
            .position(|t| punctuation(t, ';'))
            .ok_or_else(invalid)?
            + 1
    } else {
        0
    };
    if !tokens.get(start).is_some_and(|t| ident(t, "struct")) {
        return Ok((tokens.into_iter().collect(), None));
    }
    let rest = &tokens[start..];
    let [
        s,
        n,
        TokenTree::Group(schema),
        f,
        new,
        TokenTree::Group(args),
        minus,
        arrow,
        ret,
        TokenTree::Group(body),
        entry @ ..,
    ] = rest
    else {
        return Err(invalid());
    };
    if !exact(&[s.clone(), n.clone()], &["struct", "PolicyParams"])
        || schema.delimiter() != Delimiter::Brace
        || !ident(f, "fn")
        || !ident(new, "new")
        || args.delimiter() != Delimiter::Parenthesis
        || !args.stream().is_empty()
        || !matches!(minus,TokenTree::Punct(p) if p.as_char()=='-' && p.spacing()==proc_macro2::Spacing::Joint)
        || !punctuation(arrow, '>')
        || !ident(ret, "PolicyParams")
        || body.delimiter() != Delimiter::Brace
    {
        return Err(invalid());
    }
    let init: Vec<_> = body.stream().into_iter().collect();
    let [name, TokenTree::Group(values)] = init.as_slice() else {
        return Err(invalid());
    };
    if !ident(name, "PolicyParams") || values.delimiter() != Delimiter::Brace {
        return Err(invalid());
    }
    let schema = fields(schema.stream())?;
    let mut values: BTreeMap<_, _> = fields(values.stream())?.into_iter().collect();
    if schema.len() != values.len() {
        return Err(invalid());
    }
    let mut bindings = BTreeMap::new();
    for (name, ty) in schema {
        let value = values.remove(&name).ok_or_else(invalid)?;
        let field_kind = kind(&ty)?;
        let binding = literal(&value, field_kind)?;
        if field_kind == Kind::OwnerLimit {
            let Expr::Array { values } = &binding else {
                return Err(invalid());
            };
            let expected = match name.as_str() {
                "daily_limit" => "native_daily_limit",
                "action_limit" => "native_action_limit",
                _ => return Err(invalid()),
            };
            if !matches!(&values[0],Expr::String{value} if value==expected) {
                return Err(invalid());
            }
        }
        bindings.insert(name, binding);
    }
    let mut storage_keys = std::collections::BTreeSet::new();
    for binding in bindings.values() {
        if let Expr::Array { values } = binding
            && let [Expr::String { value: key }, Expr::Integer { .. }] = values.as_slice()
            && !storage_keys.insert(key)
        {
            return Err(invalid());
        }
    }
    let entry = tokens[..start]
        .iter()
        .cloned()
        .chain(entry.iter().cloned())
        .collect();
    Ok((entry, Some(bindings)))
}

/// Return constructor initializers after checking the complete source and storage descriptors.
pub fn native_storage_initializers(source: &str) -> Result<BTreeMap<String, u64>, CompileError> {
    crate::compile(source)?;
    let tokens = source.parse::<TokenStream>().map_err(|_| invalid())?;
    let (_, params) = extract(&tokens)?;
    let mut result = BTreeMap::new();
    for value in params.unwrap_or_default().values() {
        if let Expr::Array { values } = value
            && let [
                Expr::String { value: key },
                Expr::Integer { value: initial },
            ] = values.as_slice()
        {
            result.insert(key.clone(), *initial);
        }
    }
    Ok(result)
}
