use crate::{
    CallSite, CompileError, CompiledPolicy, Expr, LANGUAGE, MAX_SOURCE_BYTES, Program,
    REGISTRY_VERSION, SourceSpan, Statement, WorkflowBlock, canonical_ir_hash, digest,
    registry::function, validate_program,
};
use proc_macro2::Span;
use std::collections::BTreeSet;
use syn::{BinOp, Expr as SynExpr, FnArg, Item, Pat, ReturnType, Stmt, Type, spanned::Spanned};

fn error(span: Span, message: impl Into<String>) -> CompileError {
    let p = span.start();
    CompileError {
        code: "INVALID_POLICY".into(),
        message: message.into(),
        line: Some(p.line),
        column: Some(p.column + 1),
    }
}
fn range(span: Span) -> SourceSpan {
    let r = span.byte_range();
    SourceSpan {
        start: r.start,
        end: r.end,
    }
}
fn simple_path(path: &syn::Path, name: &str) -> bool {
    path.leading_colon.is_none()
        && path.segments.len() == 1
        && path.segments[0].ident == name
        && path.segments[0].arguments.is_empty()
}
fn annotation(ty: &Type) -> Option<String> {
    match ty {
        Type::Path(p)
            if p.qself.is_none()
                && p.path.segments.len() == 1
                && p.path.segments[0].arguments.is_empty() =>
        {
            Some(p.path.segments[0].ident.to_string())
        }
        Type::Reference(r)
            if r.mutability.is_none()
                && r.lifetime.is_none()
                && matches!(&*r.elem,Type::Path(p) if simple_path(&p.path,"str")) =>
        {
            Some("&str".into())
        }
        _ => None,
    }
}

struct Parser {
    nodes: usize,
}
impl Parser {
    fn tick(&mut self, span: Span, depth: usize) -> Result<(), CompileError> {
        self.nodes += 1;
        if self.nodes > crate::MAX_NODES || depth > crate::MAX_DEPTH {
            Err(error(
                span,
                "Policy complexity exceeds the supported limit.",
            ))
        } else {
            Ok(())
        }
    }
    fn block(&mut self, block: &syn::Block, depth: usize) -> Result<Vec<Statement>, CompileError> {
        block
            .stmts
            .iter()
            .map(|s| self.statement(s, depth + 1))
            .collect()
    }
    fn statement(&mut self, stmt: &Stmt, depth: usize) -> Result<Statement, CompileError> {
        self.tick(stmt.span(), depth)?;
        let span = range(stmt.span());
        Ok(match stmt {
            Stmt::Local(local) => {
                if !local.attrs.is_empty() {
                    return Err(error(
                        local.span(),
                        "Attributes are not supported in policies.",
                    ));
                }
                let (pat, ty) = match &local.pat {
                    Pat::Type(p) => (
                        &*p.pat,
                        Some(
                            annotation(&p.ty)
                                .ok_or_else(|| error(p.ty.span(), "Unsupported variable type."))?,
                        ),
                    ),
                    p => (p, None),
                };
                let Pat::Ident(ident) = pat else {
                    return Err(error(pat.span(), "Use a simple immutable variable name."));
                };
                if ident.mutability.is_some() || ident.by_ref.is_some() || ident.subpat.is_some() {
                    return Err(error(pat.span(), "Variables must be immutable."));
                }
                let init = local
                    .init
                    .as_ref()
                    .ok_or_else(|| error(local.span(), "Variables require an initial value."))?;
                if init.diverge.is_some() {
                    return Err(error(local.span(), "let-else is not supported."));
                }
                Statement::Let {
                    name: ident.ident.to_string(),
                    value: self.expr(&init.expr, depth + 1)?,
                    annotation: ty,
                    span,
                }
            }
            Stmt::Expr(SynExpr::Return(ret), _) => Statement::Return {
                value: self.expr(
                    ret.expr
                        .as_ref()
                        .ok_or_else(|| error(ret.span(), "Return Ok(()) or fail(\"reason\")."))?,
                    depth + 1,
                )?,
                span,
            },
            Stmt::Expr(SynExpr::If(i), _) => self.if_statement(i, depth + 1)?,
            Stmt::Expr(expr, semi) => Statement::Expression {
                value: self.expr(expr, depth + 1)?,
                semicolon: semi.is_some(),
                span,
            },
            _ => {
                return Err(error(
                    stmt.span(),
                    "Only immutable variables, predefined calls, if statements and returns are supported.",
                ));
            }
        })
    }
    fn if_statement(&mut self, i: &syn::ExprIf, depth: usize) -> Result<Statement, CompileError> {
        self.tick(i.span(), depth)?;
        if !i.attrs.is_empty() {
            return Err(error(i.span(), "Attributes are not supported."));
        }
        let else_branch = if let Some((_, branch)) = &i.else_branch {
            match &**branch {
                SynExpr::Block(b) if b.label.is_none() && b.attrs.is_empty() => {
                    self.block(&b.block, depth + 1)?
                }
                SynExpr::If(other) => vec![self.if_statement(other, depth + 1)?],
                _ => return Err(error(branch.span(), "Unsupported else branch.")),
            }
        } else {
            vec![]
        };
        Ok(Statement::If {
            condition: self.expr(&i.cond, depth + 1)?,
            then_branch: self.block(&i.then_branch, depth + 1)?,
            else_branch,
            span: range(i.span()),
        })
    }
    fn expr(&mut self, expr: &SynExpr, depth: usize) -> Result<Expr, CompileError> {
        self.tick(expr.span(), depth)?;
        let has_attrs = match expr {
            SynExpr::Lit(e) => !e.attrs.is_empty(),
            SynExpr::Path(e) => !e.attrs.is_empty(),
            SynExpr::Field(e) => !e.attrs.is_empty(),
            SynExpr::Binary(e) => !e.attrs.is_empty(),
            SynExpr::Unary(e) => !e.attrs.is_empty(),
            SynExpr::Try(e) => !e.attrs.is_empty(),
            SynExpr::Await(e) => !e.attrs.is_empty(),
            SynExpr::Call(e) => !e.attrs.is_empty(),
            SynExpr::Reference(e) => !e.attrs.is_empty(),
            SynExpr::Array(e) => !e.attrs.is_empty(),
            SynExpr::Paren(e) => !e.attrs.is_empty(),
            SynExpr::Tuple(e) => !e.attrs.is_empty(),
            _ => false,
        };
        if has_attrs {
            return Err(error(expr.span(), "Attributes are not supported."));
        }
        Ok(match expr {
            SynExpr::Lit(lit) => match &lit.lit {
                syn::Lit::Str(s) => Expr::String { value: s.value() },
                syn::Lit::Int(i) if i.suffix().is_empty() || i.suffix() == "u64" => Expr::Integer {
                    value: i
                        .base10_parse::<u64>()
                        .map_err(|_| error(i.span(), "Integers must fit in u64."))?,
                },
                syn::Lit::Bool(b) => Expr::Boolean { value: b.value },
                _ => {
                    return Err(error(
                        lit.span(),
                        "Use strings, booleans or non-negative u64 integers.",
                    ));
                }
            },
            SynExpr::Path(p)
                if p.qself.is_none()
                    && p.path.leading_colon.is_none()
                    && p.path.segments.len() == 1
                    && p.path.segments[0].arguments.is_empty() =>
            {
                Expr::Variable {
                    name: p.path.segments[0].ident.to_string(),
                }
            }
            SynExpr::Tuple(t) if t.elems.is_empty() => Expr::Unit,
            SynExpr::Paren(p) => self.expr(&p.expr, depth + 1)?,
            SynExpr::Reference(r)
                if r.mutability.is_none() && matches!(&*r.expr, SynExpr::Array(_)) =>
            {
                let SynExpr::Array(a) = &*r.expr else {
                    unreachable!()
                };
                Expr::Array {
                    values: a
                        .elems
                        .iter()
                        .map(|e| self.expr(e, depth + 1))
                        .collect::<Result<_, _>>()?,
                }
            }
            SynExpr::Field(f) => {
                let syn::Member::Named(name) = &f.member else {
                    return Err(error(
                        f.span(),
                        "Only named context and confidence fields are supported.",
                    ));
                };
                Expr::Field {
                    object: Box::new(self.expr(&f.base, depth + 1)?),
                    name: name.to_string(),
                }
            }
            SynExpr::Binary(b) => Expr::Binary {
                op: match b.op {
                    BinOp::Add(_) => "+",
                    BinOp::Sub(_) => "-",
                    BinOp::Mul(_) => "*",
                    BinOp::Div(_) => "/",
                    BinOp::Rem(_) => "%",
                    BinOp::Eq(_) => "==",
                    BinOp::Ne(_) => "!=",
                    BinOp::Gt(_) => ">",
                    BinOp::Ge(_) => ">=",
                    BinOp::Lt(_) => "<",
                    BinOp::Le(_) => "<=",
                    BinOp::And(_) => "&&",
                    BinOp::Or(_) => "||",
                    _ => return Err(error(b.op.span(), "This operator is not supported.")),
                }
                .into(),
                left: Box::new(self.expr(&b.left, depth + 1)?),
                right: Box::new(self.expr(&b.right, depth + 1)?),
            },
            SynExpr::Unary(u) if matches!(u.op, syn::UnOp::Not(_)) => Expr::Not {
                value: Box::new(self.expr(&u.expr, depth + 1)?),
            },
            SynExpr::Try(t) => Expr::Try {
                value: Box::new(self.expr(&t.expr, depth + 1)?),
            },
            SynExpr::Await(a) => Expr::Await {
                value: Box::new(self.expr(&a.base, depth + 1)?),
            },
            SynExpr::Call(c) => {
                let SynExpr::Path(p) = &*c.func else {
                    return Err(error(
                        c.func.span(),
                        "Only direct predefined function calls are supported.",
                    ));
                };
                if p.qself.is_some()
                    || p.path.leading_colon.is_some()
                    || p.path.segments.len() != 1
                    || !p.path.segments[0].arguments.is_empty()
                {
                    return Err(error(
                        p.span(),
                        "Only direct predefined function calls are supported.",
                    ));
                }
                Expr::Call {
                    name: p.path.segments[0].ident.to_string(),
                    args: c
                        .args
                        .iter()
                        .map(|e| self.expr(e, depth + 1))
                        .collect::<Result<_, _>>()?,
                    span: range(p.span()),
                }
            }
            _ => {
                return Err(error(
                    expr.span(),
                    "This Rust construct is outside the AllowIt policy subset.",
                ));
            }
        })
    }
}

fn valid_import(use_item: &syn::ItemUse) -> bool {
    if !use_item.attrs.is_empty()
        || use_item.leading_colon.is_some()
        || !matches!(use_item.vis, syn::Visibility::Inherited)
    {
        return false;
    }
    matches!(&use_item.tree,syn::UseTree::Path(a) if a.ident=="allowit" && matches!(&*a.tree,syn::UseTree::Path(p) if p.ident=="prelude" && matches!(&*p.tree,syn::UseTree::Glob(_))))
}
fn offset(source: &str, byte: usize) -> usize {
    source[..byte.min(source.len())].encode_utf16().count()
}
fn source_slice(source: &str, span: SourceSpan) -> String {
    source.get(span.start..span.end).unwrap_or("").to_string()
}
fn direct_call(expr: &Expr) -> Option<(&str, &[Expr])> {
    match expr {
        Expr::Try { value } | Expr::Await { value } => direct_call(value),
        Expr::Call { name, args, .. } if name != "Ok" => Some((name, args)),
        _ => None,
    }
}
fn literal(expr: &Expr) -> bool {
    match expr {
        Expr::String { .. } | Expr::Integer { .. } | Expr::Boolean { .. } => true,
        Expr::Variable { name } => name == "ctx",
        Expr::Array { values } => values.iter().all(literal),
        _ => false,
    }
}
fn argument(expr: &Expr) -> String {
    match expr {
        Expr::String { value } => value.clone(),
        Expr::Integer { value } => value.to_string(),
        Expr::Boolean { value } => value.to_string(),
        Expr::Array { values } => values.iter().map(argument).collect::<Vec<_>>().join(", "),
        _ => String::new(),
    }
}
fn walk_expr(expr: &Expr, calls: &mut Vec<(String, SourceSpan)>) {
    match expr {
        Expr::Call { name, args, span } => {
            if name != "Ok" {
                calls.push((name.clone(), *span));
            }
            for arg in args {
                walk_expr(arg, calls);
            }
        }
        Expr::Try { value } | Expr::Await { value } | Expr::Not { value } => {
            walk_expr(value, calls)
        }
        Expr::Field { object, .. } => walk_expr(object, calls),
        Expr::Binary { left, right, .. } => {
            walk_expr(left, calls);
            walk_expr(right, calls);
        }
        Expr::Array { values } => {
            for value in values {
                walk_expr(value, calls);
            }
        }
        _ => {}
    }
}
fn walk_block(block: &[Statement], calls: &mut Vec<(String, SourceSpan)>) {
    for s in block {
        match s {
            Statement::Let { value, .. }
            | Statement::Return { value, .. }
            | Statement::Expression { value, .. } => walk_expr(value, calls),
            Statement::If {
                condition,
                then_branch,
                else_branch,
                ..
            } => {
                walk_expr(condition, calls);
                walk_block(then_branch, calls);
                walk_block(else_branch, calls);
            }
        }
    }
}

/// Parse source as Rust, reject unsupported syntax, type/effect-check every branch and
/// produce the sole executable IR and its source-preserving workflow projection.
pub fn compile(source: &str) -> Result<CompiledPolicy, CompileError> {
    struct SpanCleanup;
    impl Drop for SpanCleanup {
        fn drop(&mut self) {
            proc_macro2::extra::invalidate_current_thread_spans();
        }
    }
    let _cleanup = SpanCleanup;
    compile_inner(source)
}

fn compile_inner(source: &str) -> Result<CompiledPolicy, CompileError> {
    if source.starts_with('\u{feff}') {
        return Err(CompileError::new(
            "INVALID_POLICY",
            "Save the policy as UTF-8 without a byte-order mark.",
        ));
    }
    if source.len() > MAX_SOURCE_BYTES {
        return Err(CompileError::new(
            "SOURCE_TOO_LARGE",
            "Policy source may contain at most 32 KiB.",
        ));
    }
    let tokens = source
        .parse::<proc_macro2::TokenStream>()
        .map_err(|e| error(e.span(), e.to_string()))?;
    validate_signature(&tokens)?;
    validate_block_shapes(&tokens)?;
    validate_token_budget(&tokens)?;
    let file = syn::parse2::<syn::File>(tokens).map_err(|e| error(e.span(), e.to_string()))?;
    if !file.attrs.is_empty() || file.shebang.is_some() {
        return Err(CompileError::new(
            "INVALID_POLICY",
            "File attributes and shebangs are not supported.",
        ));
    }
    let mut policy_fn = None;
    let mut imported = false;
    for item in &file.items {
        match item {
            Item::Use(u) if !imported && valid_import(u) => imported = true,
            Item::Fn(f) if policy_fn.is_none() => policy_fn = Some(f),
            _ => {
                return Err(error(
                    item.span(),
                    "A policy contains only an optional allowit::prelude import and the evaluate function.",
                ));
            }
        }
    }
    let f = policy_fn.ok_or_else(|| {
        CompileError::new(
            "INVALID_POLICY",
            "Define pub async fn evaluate(ctx: &Context) -> PolicyResult.",
        )
    })?;
    let sig = &f.sig;
    if !f.attrs.is_empty()
        || sig.ident != "evaluate"
        || sig.asyncness.is_none()
        || sig.constness.is_some()
        || sig.unsafety.is_some()
        || sig.abi.is_some()
        || sig.variadic.is_some()
        || !sig.generics.params.is_empty()
        || sig.generics.where_clause.is_some()
        || !matches!(f.vis, syn::Visibility::Public(_))
        || sig.inputs.len() != 1
    {
        return Err(error(
            sig.span(),
            "Use pub async fn evaluate(ctx: &Context) -> PolicyResult.",
        ));
    }
    let valid_arg = matches!(&sig.inputs[0],FnArg::Typed(arg) if arg.attrs.is_empty()&&matches!(&*arg.pat,Pat::Ident(p) if p.ident=="ctx"&&p.mutability.is_none()&&p.by_ref.is_none()&&p.subpat.is_none())&&matches!(&*arg.ty,Type::Reference(r) if r.mutability.is_none()&&r.lifetime.is_none()&&matches!(&*r.elem,Type::Path(p) if p.qself.is_none()&&simple_path(&p.path,"Context"))));
    let valid_return = matches!(&sig.output,ReturnType::Type(_,ty) if matches!(&**ty,Type::Path(p) if p.qself.is_none()&&simple_path(&p.path,"PolicyResult")));
    if !valid_arg || !valid_return {
        return Err(error(
            sig.span(),
            "Use pub async fn evaluate(ctx: &Context) -> PolicyResult.",
        ));
    }
    let mut parser = Parser { nodes: 0 };
    let ir = Program {
        version: REGISTRY_VERSION.into(),
        statements: parser.block(&f.block, 0)?,
    };
    validate_program(&ir)?;
    let source_hash = digest(source.as_bytes());
    let ir_hash = canonical_ir_hash(&ir)?;
    let mut workflow: Vec<WorkflowBlock> = vec![];
    let mut limit = String::new();
    for statement in &ir.statements {
        let span = statement.span();
        let value = match statement {
            Statement::Expression { value, .. } | Statement::Return { value, .. } => Some(value),
            _ => None,
        };
        let predefined = value
            .and_then(direct_call)
            .filter(|(_, args)| args.iter().all(literal));
        let pass = matches!(value,Some(Expr::Call{name,..}) if name=="Ok");
        let (kind, name, label, description, arguments) = if let Some((name, args)) = predefined {
            let info = function(name).expect("validator checked the registry");
            if name == "set_cap" {
                limit = argument(&args[1]);
            }
            (
                "function",
                name.to_string(),
                info.title,
                info.description,
                args.iter()
                    .filter(|a| !matches!(a,Expr::Variable{name} if name=="ctx"))
                    .map(argument)
                    .collect(),
            )
        } else if pass {
            (
                "pass",
                "Ok".into(),
                "Pass the policy check".into(),
                "The request meets the policy's rules. This result is not a transfer receipt."
                    .into(),
                vec![],
            )
        } else {
            ("custom","custom".into(),"Custom code".into(),"These conditions and calculations run in the order shown. Open the code to inspect their exact rules.".into(),vec![])
        };
        if kind == "custom" && workflow.last().is_some_and(|b| b.kind == "custom") {
            let last = workflow.last_mut().expect("checked");
            last.end = offset(source, span.end);
            let start = source
                .char_indices()
                .scan(0usize, |u, (byte, c)| {
                    let old = *u;
                    *u += c.len_utf16();
                    Some((old, byte))
                })
                .find(|(u, _)| *u == last.start)
                .map(|(_, b)| b)
                .unwrap_or(0);
            last.source = source.get(start..span.end).unwrap_or("").into();
            continue;
        }
        workflow.push(WorkflowBlock {
            id: digest(format!("{source_hash}:{}:{kind}", span.start).as_bytes())[..16].into(),
            kind: kind.into(),
            name,
            label,
            description,
            arguments,
            source: source_slice(source, span),
            start: offset(source, span.start),
            end: offset(source, span.end),
        });
    }
    let mut found = vec![];
    walk_block(&ir.statements, &mut found);
    found.sort_by_key(|(_, s)| s.start);
    let mut seen = BTreeSet::new();
    let calls = found
        .into_iter()
        .filter(|(_, s)| seen.insert(s.start))
        .map(|(name, span)| CallSite {
            name,
            start: offset(source, span.start),
            end: offset(source, span.end),
        })
        .collect();
    Ok(CompiledPolicy {
        language: LANGUAGE.into(),
        source_hash,
        ir_hash,
        registry_version: REGISTRY_VERSION.into(),
        limit,
        token: "USDC".into(),
        source: source.into(),
        workflow,
        calls,
        ir,
    })
}

// Validate the only supported item/signature without invoking syn's recursive type parser.
// Type aliases, generic parameters, nested references and function-pointer types are not DSL
// features, so they must never reach that parser even when their token count is small.
fn validate_signature(tokens: &proc_macro2::TokenStream) -> Result<(), CompileError> {
    use proc_macro2::{Delimiter, TokenTree};
    fn invalid() -> CompileError {
        CompileError::new(
            "INVALID_POLICY",
            "Use an optional allowit::prelude import and exactly pub async fn evaluate(ctx: &Context) -> PolicyResult { ... }.",
        )
    }
    fn matches(token: Option<TokenTree>, expected: &str) -> bool {
        match token {
            Some(TokenTree::Ident(ident)) => ident == expected,
            Some(TokenTree::Punct(punct)) => {
                expected.len() == 1 && expected.starts_with(punct.as_char())
            }
            _ => false,
        }
    }
    fn expect(
        iter: &mut proc_macro2::token_stream::IntoIter,
        expected: &[&str],
    ) -> Result<(), CompileError> {
        for word in expected {
            if !matches(iter.next(), word) {
                return Err(invalid());
            }
        }
        Ok(())
    }
    let mut iter = tokens.clone().into_iter();
    let first = iter.next();
    if matches(first.clone(), "use") {
        expect(
            &mut iter,
            &["allowit", ":", ":", "prelude", ":", ":", "*", ";", "pub"],
        )?;
    } else if !matches(first, "pub") {
        return Err(invalid());
    }
    expect(&mut iter, &["async", "fn", "evaluate"])?;
    let Some(TokenTree::Group(params)) = iter.next() else {
        return Err(invalid());
    };
    if params.delimiter() != Delimiter::Parenthesis {
        return Err(invalid());
    }
    let mut params = params.stream().into_iter();
    expect(&mut params, &["ctx", ":", "&", "Context"])?;
    if let Some(last) = params.next()
        && (!matches(Some(last), ",") || params.next().is_some())
    {
        return Err(invalid());
    }
    expect(&mut iter, &["-", ">", "PolicyResult"])?;
    if !matches!(iter.next(), Some(TokenTree::Group(body)) if body.delimiter() == Delimiter::Brace)
        || iter.next().is_some()
    {
        return Err(invalid());
    }
    Ok(())
}

// syn creates deep ASTs for shallow postfix chains. Count tokens iteratively before parsing;
// parentheses/brackets contribute to their enclosing expression rather than resetting its budget.
fn validate_token_budget(tokens: &proc_macro2::TokenStream) -> Result<(), CompileError> {
    use proc_macro2::{Delimiter, TokenTree};
    enum Pending {
        Tokens(proc_macro2::token_stream::IntoIter, bool),
        End(Delimiter),
    }
    let mut stack = vec![Pending::Tokens(tokens.clone().into_iter(), true)];
    let (mut count, mut operators, mut flow, mut elses, mut total) =
        (0usize, 0usize, 0usize, 0usize, 0usize);
    let limit = || {
        CompileError::new(
            "RESOURCE_LIMIT",
            "A policy expression exceeds the parser resource limit (256 tokens, 96 operators, 32 control prefixes, or 32 else branches). Split the conditions into separate statements.",
        )
    };
    let total_limit = || {
        CompileError::new(
            "RESOURCE_LIMIT",
            "A policy may contain at most 1,024 syntax tokens, including delimiters. Simplify the policy or split it into separate policies.",
        )
    };
    while let Some(pending) = stack.pop() {
        let (token, statements) = match pending {
            Pending::End(Delimiter::Brace) => {
                total += 1;
                if total > 1024 {
                    return Err(total_limit());
                }
                count = 0;
                operators = 0;
                flow = 0;
                continue;
            }
            Pending::End(_) => {
                total += 1;
                if total > 1024 {
                    return Err(total_limit());
                }
                count += 1;
                if count > 256 {
                    return Err(limit());
                }
                continue;
            }
            Pending::Tokens(mut iter, statements) => match iter.next() {
                Some(token) => {
                    stack.push(Pending::Tokens(iter, statements));
                    (token, statements)
                }
                None => continue,
            },
        };
        total += 1;
        if total > 1024 {
            return Err(total_limit());
        }
        count += 1;
        match token {
            TokenTree::Group(group) => {
                if group.delimiter() == Delimiter::Brace {
                    count = 0;
                    operators = 0;
                    flow = 0;
                }
                stack.push(Pending::End(group.delimiter()));
                stack.push(Pending::Tokens(
                    group.stream().into_iter(),
                    group.delimiter() == Delimiter::Brace,
                ));
            }
            TokenTree::Punct(p) if p.as_char() == ';' && statements => {
                count = 0;
                operators = 0;
                flow = 0;
            }
            TokenTree::Punct(p) if "!+-*/%&|<>=?.".contains(p.as_char()) => operators += 1,
            TokenTree::Ident(ident) => {
                let name = ident.to_string();
                if ["if", "return", "break", "yield"].contains(&name.as_str()) {
                    flow += 1;
                }
                if name == "else" {
                    elses += 1;
                }
            }
            _ => {}
        }
        if count > 256 || operators > 96 || flow > 32 || elses > 32 {
            return Err(limit());
        }
    }
    Ok(())
}

// The subset has braces only for the evaluate body and if/else bodies. Reject expression
// blocks before syn: otherwise f({})({})... could repeatedly reset a flat token budget while
// still constructing a deep postfix AST. Each nested group is visited iteratively.
fn validate_block_shapes(tokens: &proc_macro2::TokenStream) -> Result<(), CompileError> {
    use proc_macro2::{Delimiter, TokenTree};
    #[derive(Clone, Copy, PartialEq)]
    enum Scope {
        File,
        Body,
        Expression,
    }
    struct Frame {
        iter: proc_macro2::token_stream::IntoIter,
        scope: Scope,
        expects_body: bool,
        statement_start: bool,
        after_if_body: bool,
        pending_else: bool,
        expects_operand: bool,
        depth: usize,
    }
    fn frame(tokens: proc_macro2::TokenStream, scope: Scope, depth: usize) -> Frame {
        Frame {
            iter: tokens.into_iter(),
            scope,
            expects_body: false,
            statement_start: true,
            after_if_body: false,
            pending_else: false,
            expects_operand: true,
            depth,
        }
    }
    let mut stack = vec![frame(tokens.clone(), Scope::File, 0)];
    let mut file_bodies = 0usize;
    while let Some(mut current) = stack.pop() {
        let Some(token) = current.iter.next() else {
            continue;
        };
        if current.scope == Scope::Body && current.after_if_body {
            match &token {
                TokenTree::Ident(ident) if ident == "else" => {
                    current.expects_body = true;
                    current.pending_else = true;
                    current.after_if_body = false;
                    current.statement_start = false;
                    stack.push(current);
                    continue;
                }
                TokenTree::Ident(ident)
                    if ident == "let"
                        || ident == "if"
                        || ident == "return"
                        || ident == "Ok"
                        || crate::registry::function(&ident.to_string()).is_some() =>
                {
                    current.after_if_body = false;
                    current.statement_start = true;
                }
                TokenTree::Punct(p) if p.as_char() == ';' => {
                    current.after_if_body = false;
                    current.statement_start = true;
                    stack.push(current);
                    continue;
                }
                _ => {
                    return Err(error(
                        token.span(),
                        "An if/else block must be followed by else or a new statement.",
                    ));
                }
            }
        }
        match token {
            TokenTree::Group(group) => {
                // The signature preflight already checked the fixed parameter group.
                if current.scope == Scope::File && group.delimiter() != Delimiter::Brace {
                    stack.push(current);
                    continue;
                }
                let depth = current.depth + 1;
                if depth > 32 {
                    return Err(error(
                        group.span(),
                        "Policy delimiter depth exceeds 32 levels.",
                    ));
                }
                let scope = if group.delimiter() == Delimiter::Brace {
                    if current.scope == Scope::File {
                        file_bodies += 1;
                        if file_bodies > 1 {
                            return Err(error(
                                group.span(),
                                "A policy contains one evaluate function.",
                            ));
                        }
                    } else if current.scope != Scope::Body || !current.expects_body {
                        return Err(error(
                            group.span(),
                            "Only the evaluate body and if/else bodies may contain code blocks.",
                        ));
                    }
                    current.expects_body = false;
                    current.pending_else = false;
                    current.after_if_body = current.scope == Scope::Body;
                    Scope::Body
                } else {
                    current.statement_start = false;
                    current.expects_operand = false;
                    Scope::Expression
                };
                stack.push(current);
                stack.push(frame(group.stream(), scope, depth));
            }
            TokenTree::Ident(ident) => {
                if current.scope != Scope::File
                    && [
                        "as", "type", "fn", "impl", "dyn", "const", "static", "struct", "enum",
                        "union", "trait", "mod", "use", "extern", "for", "while", "loop", "match",
                        "unsafe", "async", "move",
                    ]
                    .contains(&ident.to_string().as_str())
                {
                    return Err(error(
                        ident.span(),
                        "Type declarations, casts and custom items are not supported.",
                    ));
                }
                if current.scope == Scope::Expression
                    && ["if", "else", "return", "break", "yield", "match"]
                        .contains(&ident.to_string().as_str())
                {
                    return Err(error(
                        ident.span(),
                        "Control flow is supported only as a policy statement.",
                    ));
                }
                if current.scope == Scope::Body {
                    if ident == "let" {
                        if !current.statement_start {
                            return Err(error(
                                ident.span(),
                                "let is supported only at the start of a statement.",
                            ));
                        }
                        validate_let_header(&mut current.iter)?;
                    } else if ident == "if" {
                        if !current.statement_start && !current.pending_else {
                            return Err(error(
                                ident.span(),
                                "if is supported only at the start of a statement or after else.",
                            ));
                        }
                        current.expects_body = true;
                        current.pending_else = false;
                    } else if ident == "else" {
                        return Err(error(
                            ident.span(),
                            "else must immediately follow an if body.",
                        ));
                    } else if ident == "return" && !current.statement_start {
                        return Err(error(
                            ident.span(),
                            "return is supported only at the start of a statement.",
                        ));
                    }
                    current.statement_start = false;
                }
                current.expects_operand = ident == "if" || ident == "return" || ident == "let";
                stack.push(current);
            }
            TokenTree::Punct(p) => {
                if current.scope != Scope::File
                    && p.as_char() == '-'
                    && matches!(current.iter.clone().next(), Some(TokenTree::Punct(next)) if next.as_char() == '>')
                {
                    return Err(error(
                        p.span(),
                        "Function types and closures are not supported.",
                    ));
                }
                if current.scope != Scope::File
                    && p.as_char() == '|'
                    && (current.expects_operand
                        || p.spacing() != proc_macro2::Spacing::Joint
                        || !matches!(current.iter.next(), Some(TokenTree::Punct(next)) if next.as_char() == '|'))
                {
                    return Err(error(
                        p.span(),
                        "Use || between boolean expressions. Closures and bitwise operators are not supported.",
                    ));
                }
                if current.scope != Scope::File && p.as_char() == ':' {
                    return Err(error(
                        p.span(),
                        "Type syntax is supported only in the fixed signature and simple let annotations.",
                    ));
                }
                if current.scope != Scope::File && p.as_char() == '<' && current.expects_operand {
                    return Err(error(
                        p.span(),
                        "Qualified type expressions are not supported.",
                    ));
                }
                if p.as_char() == ';' {
                    if current.scope == Scope::Expression {
                        return Err(error(
                            p.span(),
                            "Semicolons are supported only between policy statements, not inside expressions.",
                        ));
                    }
                    current.expects_body = false;
                    current.pending_else = false;
                    current.statement_start = true;
                } else {
                    current.statement_start = false;
                }
                current.expects_operand = p.as_char() != '?';
                stack.push(current);
            }
            _ => {
                current.statement_start = false;
                current.expects_operand = false;
                stack.push(current);
            }
        }
    }
    Ok(())
}

// Consume only the simple let header, leaving its initializer to the expression validator.
// All other colons are rejected before syn, so recursive types cannot enter through a local
// annotation, closure parameter, label, cast, turbofish or qualified path.
fn validate_let_header(iter: &mut proc_macro2::token_stream::IntoIter) -> Result<(), CompileError> {
    use proc_macro2::TokenTree;
    let invalid = || {
        CompileError::new(
            "INVALID_POLICY",
            "Use let name = value, optionally annotated with u64, bool, &str or ConfidenceInterval.",
        )
    };
    if !matches!(iter.next(), Some(TokenTree::Ident(_))) {
        return Err(invalid());
    }
    match iter.next() {
        Some(TokenTree::Punct(p)) if p.as_char() == '=' => return Ok(()),
        Some(TokenTree::Punct(p)) if p.as_char() == ':' => {}
        _ => return Err(invalid()),
    }
    match iter.next() {
        Some(TokenTree::Ident(ident))
            if ["u64", "bool", "ConfidenceInterval"].contains(&ident.to_string().as_str()) => {}
        Some(TokenTree::Punct(p)) if p.as_char() == '&' => {
            if !matches!(iter.next(), Some(TokenTree::Ident(ident)) if ident == "str") {
                return Err(invalid());
            }
        }
        _ => return Err(invalid()),
    }
    if !matches!(iter.next(), Some(TokenTree::Punct(p)) if p.as_char() == '=') {
        return Err(invalid());
    }
    Ok(())
}
