//! Source-preserving parameter edits. Only AST-selected argument/literal spans change.
use crate::{CompileError, CompiledPolicy, ScoreThreshold, WorkflowBlock};
use serde::Deserialize;
use std::ops::Range;
use syn::{Expr, Item, ItemFn, Stmt, spanned::Spanned, visit::Visit};

fn bad(message: &str) -> CompileError {
    CompileError::new("INVALID_EDIT", message)
}
fn path(expr: &Expr, name: &str) -> bool {
    matches!(unparen(expr), Expr::Path(p) if p.path.is_ident(name))
}
fn unparen(expr: &Expr) -> &Expr {
    match expr {
        Expr::Paren(p) => unparen(&p.expr),
        Expr::Group(g) => unparen(&g.expr),
        _ => expr,
    }
}
fn call<'a>(expr: &'a Expr, name: &str) -> Option<&'a syn::ExprCall> {
    match unparen(expr) {
        Expr::Call(c) if path(&c.func, name) => Some(c),
        Expr::Try(t) => call(&t.expr, name),
        Expr::Await(a) => call(&a.base, name),
        _ => None,
    }
}
fn percent(value: u64) -> String {
    format!("{}.{:02}", value / 100, value % 100)
}
fn score(value: u64) -> String {
    format!("{}.{:04}", value / 10_000, value % 10_000)
}
fn offset(source: &str, byte: usize) -> usize {
    source[..byte].encode_utf16().count()
}

struct Bound {
    info: ScoreThreshold,
    range: Range<usize>,
    percent: bool,
}
struct Bounds<'a> {
    variable: &'a str,
    found: Vec<Bound>,
}
impl<'ast> Visit<'ast> for Bounds<'_> {
    fn visit_expr_binary(&mut self, expr: &'ast syn::ExprBinary) {
        let operator = match expr.op {
            syn::BinOp::Lt(_) => "<",
            syn::BinOp::Le(_) => "<=",
            syn::BinOp::Gt(_) => ">",
            syn::BinOp::Ge(_) => ">=",
            _ => "",
        };
        if let Expr::Field(f) = unparen(&expr.left)
            && path(&f.base, self.variable)
            && let syn::Member::Named(field) = &f.member
            && (field == "lower_bps" || field == "upper_bps")
            && !operator.is_empty()
        {
            let literal = match unparen(&expr.right) {
                Expr::Lit(l) => match &l.lit {
                    syn::Lit::Int(i) => i
                        .base10_parse::<u64>()
                        .ok()
                        .map(|v| (v, i.span().byte_range(), false)),
                    _ => None,
                },
                rhs => call(rhs, "percent").and_then(|c| match c.args.first().map(unparen) {
                    Some(Expr::Lit(l)) => match &l.lit {
                        syn::Lit::Str(s) => crate::readability::percentage_bps(&s.value())
                            .ok()
                            .map(|v| (v, s.span().byte_range(), true)),
                        _ => None,
                    },
                    _ => None,
                }),
            };
            if let Some((value_bps, range, percent)) = literal
                && value_bps <= 10_000
            {
                self.found.push(Bound {
                    info: ScoreThreshold {
                        field: field.to_string(),
                        operator: operator.into(),
                        value_bps,
                    },
                    range,
                    percent,
                });
            }
        }
        syn::visit::visit_expr_binary(self, expr);
    }
}

fn bounds(function: &ItemFn, start: usize) -> Vec<Bound> {
    for (index, stmt) in function.block.stmts.iter().enumerate() {
        if stmt.span().byte_range().start != start {
            continue;
        }
        let Stmt::Local(local) = stmt else {
            return vec![];
        };
        let pattern = if let syn::Pat::Type(t) = &local.pat {
            &*t.pat
        } else {
            &local.pat
        };
        let syn::Pat::Ident(ident) = pattern else {
            return vec![];
        };
        if local
            .init
            .as_ref()
            .and_then(|i| call(&i.expr, "semantic"))
            .is_none()
        {
            return vec![];
        }
        // The validator forbids shadowing this binding in subsequent nested scopes.
        let variable = ident.ident.to_string();
        let mut visitor = Bounds {
            variable: &variable,
            found: vec![],
        };
        for later in &function.block.stmts[index + 1..] {
            visitor.visit_stmt(later);
        }
        return visitor.found;
    }
    vec![]
}

pub(crate) fn annotate(source: &str, function: &ItemFn, workflow: &mut [WorkflowBlock]) {
    for step in workflow.iter_mut().filter(|s| s.name == "semantic") {
        if let Some(stmt) = function
            .block
            .stmts
            .iter()
            .find(|s| offset(source, s.span().byte_range().start) == step.start)
        {
            step.score_thresholds = bounds(function, stmt.span().byte_range().start)
                .into_iter()
                .map(|b| b.info)
                .collect();
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Settings {
    step_id: String,
    auto_approve: bool,
    approve_percent: String,
    auto_deny: bool,
    deny_percent: String,
}

fn function(file: &syn::File) -> Result<&ItemFn, CompileError> {
    file.items
        .iter()
        .find_map(|i| if let Item::Fn(f) = i { Some(f) } else { None })
        .ok_or_else(|| bad("Policy function missing."))
}
fn parse(source: &str) -> Result<syn::File, CompileError> {
    syn::parse_file(source).map_err(|_| bad("Invalid Rust source."))
}
fn apply(source: &str, mut edits: Vec<(Range<usize>, String)>) -> Result<String, CompileError> {
    edits.sort_by_key(|(r, _)| r.start);
    if edits.windows(2).any(|e| e[0].0.end > e[1].0.start) {
        return Err(bad("Overlapping parameter spans."));
    }
    let mut result = source.to_string();
    for (range, value) in edits.into_iter().rev() {
        result.replace_range(range, &value);
    }
    crate::compile(&result)?;
    Ok(result)
}

pub(crate) fn edit_preference(
    source: &str,
    policy: &CompiledPolicy,
    settings: Settings,
) -> Result<String, CompileError> {
    let step = policy
        .workflow
        .iter()
        .find(|s| s.id == settings.step_id && s.name == "check_preference")
        .ok_or_else(|| bad("Preference step is stale or unsupported."))?;
    let above = crate::readability::percentage_bps(&settings.approve_percent)?;
    let below = crate::readability::percentage_bps(&settings.deny_percent)?;
    if settings.auto_deny && settings.auto_approve && below >= above {
        return Err(bad("Denial must be below approval."));
    }
    let file = parse(source)?;
    let stmt = function(&file)?
        .block
        .stmts
        .iter()
        .find(|s| offset(source, s.span().byte_range().start) == step.start)
        .ok_or_else(|| bad("Preference source not found."))?;
    let Stmt::Expr(expr, _) = stmt else {
        return Err(bad("Unsupported preference statement."));
    };
    let c = call(expr, "check_preference").ok_or_else(|| bad("Unsupported preference call."))?;
    let old_approve = step.arguments[1] == "true";
    let old_above = crate::readability::percentage_bps(&step.arguments[2])?;
    let old_deny = step.arguments[3] == "true";
    let old_below = crate::readability::percentage_bps(&step.arguments[4])?;
    let values = if c.args.len() == 4 {
        vec![
            (
                2,
                if settings.auto_deny {
                    score(below)
                } else {
                    "None".into()
                },
            ),
            (
                3,
                if settings.auto_approve {
                    score(above)
                } else {
                    "None".into()
                },
            ),
        ]
    } else {
        vec![
            (2, settings.auto_approve.to_string()),
            (3, format!("\"{}\"", percent(above))),
            (4, settings.auto_deny.to_string()),
            (5, format!("\"{}\"", percent(below))),
        ]
    };
    apply(
        source,
        values
            .into_iter()
            .filter(|(i, _)| {
                if c.args.len() == 4 {
                    if *i == 2 {
                        old_deny != settings.auto_deny || (settings.auto_deny && old_below != below)
                    } else {
                        old_approve != settings.auto_approve
                            || (settings.auto_approve && old_above != above)
                    }
                } else {
                    match i {
                        2 => old_approve != settings.auto_approve,
                        3 => old_above != above,
                        4 => old_deny != settings.auto_deny,
                        _ => old_below != below,
                    }
                }
            })
            .map(|(i, v)| (c.args[i].span().byte_range(), v))
            .collect(),
    )
}

pub(crate) fn edit_score_thresholds(
    source: &str,
    policy: &CompiledPolicy,
    step_id: &str,
    values: &[u64],
) -> Result<String, CompileError> {
    let step = policy
        .workflow
        .iter()
        .find(|s| s.id == step_id && s.name == "semantic")
        .ok_or_else(|| bad("Assessment step is stale or unsupported."))?;
    let file = parse(source)?;
    let function = function(&file)?;
    let stmt = function
        .block
        .stmts
        .iter()
        .find(|s| offset(source, s.span().byte_range().start) == step.start)
        .ok_or_else(|| bad("Assessment source not found."))?;
    let found = bounds(function, stmt.span().byte_range().start);
    if found.is_empty() || found.len() != values.len() || values.iter().any(|v| *v > 10_000) {
        return Err(bad(
            "Supply every editable score bound in 0..10000 basis points.",
        ));
    }
    apply(
        source,
        found
            .into_iter()
            .zip(values)
            .filter(|(b, v)| b.info.value_bps != **v)
            .map(|(b, v)| {
                (
                    b.range,
                    if b.percent {
                        format!("\"{}\"", percent(*v))
                    } else {
                        v.to_string()
                    },
                )
            })
            .collect(),
    )
}
