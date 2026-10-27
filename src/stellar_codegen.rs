//! Static native-Rust lowering for the admitted Stellar transfer policy profile.
//! The custody artifact contains generated checks, never an IR interpreter.
use crate::{CompileError, Expr, Statement, compile};
use std::collections::BTreeMap;

pub const STELLAR_ARGUMENT_SCHEMA: &[u8] = b"allowit.stellar.sep41_transfer.v1:ScValMap(amount:i128,from:Address,to:Address,token:Address)";
#[derive(Clone, Debug)]
pub struct StellarPolicyContext {
    pub asset: String,
    pub recipient: String,
    pub token: String,
    pub decimals: u32,
    pub amount: i128,
    pub spent_daily: i128,
    pub spent_total: i128,
    pub calls: u64,
    pub now: u64,
    pub spent_day: u64,
}
#[derive(Clone, Debug)]
enum Guard {
    Pass,
    Fail,
    Transaction(i128, String),
    Daily(i128, String),
    Recipient(String),
    Asset(String),
    Actions(Vec<String>),
    Not(Box<Guard>),
    And(Box<Guard>, Box<Guard>),
    Or(Box<Guard>, Box<Guard>),
}
#[derive(Clone, Debug)]
enum Step {
    Check(Guard),
    Return(Guard),
    If(Guard, Vec<Step>, Vec<Step>),
}
#[derive(Clone, Debug)]
pub struct StellarPolicy {
    pub source_hash: [u8; 32],
    pub argument_schema_hash: [u8; 32],
    pub rust_source: String,
    steps: Vec<Step>,
    mandatory: Vec<Guard>,
}
fn unsupported() -> CompileError {
    CompileError::new(
        "UNSUPPORTED_STELLAR_POLICY",
        "The native Stellar transfer profile cannot enforce this source expression. Use admitted local guards.",
    )
}
fn digest(bytes: &[u8]) -> [u8; 32] {
    use sha2::{Digest, Sha256};
    Sha256::digest(bytes).into()
}
fn literal(expr: &Expr) -> Option<&str> {
    match expr {
        Expr::String { value } => Some(value),
        _ => None,
    }
}
fn context(expr: &Expr) -> bool {
    matches!(expr,Expr::Variable{name} if name=="ctx")
}
fn field(expr: &Expr, key: &str) -> bool {
    matches!(expr,Expr::Field{object,name} if context(object)&&name==key)
}
fn units(expr: &Expr) -> Result<i128, CompileError> {
    match expr {
        Expr::Integer { value } => Ok(i128::from(*value) * 10),
        Expr::String { value } => Ok(i128::from(crate::validation::amount_units(value)?) * 10),
        _ => Err(unsupported()),
    }
}
fn native_guard(expr: &Expr, bindings: &BTreeMap<String, Guard>) -> Result<Guard, CompileError> {
    match expr {
        Expr::Try { value } => native_guard(value, bindings),
        Expr::Boolean { value } => Ok(if *value { Guard::Pass } else { Guard::Fail }),
        Expr::Variable { name } => bindings.get(name).cloned().ok_or_else(unsupported),
        Expr::Not { value } => Ok(Guard::Not(Box::new(native_guard(value, bindings)?))),
        Expr::Binary { op, left, right }
            if op == "<="
                && field(left, "amount_units")
                && matches!(&**right, Expr::Integer { .. }) =>
        {
            Ok(Guard::Transaction(units(right)?, "USDC".into()))
        }
        Expr::Binary { op, left, right } if op == "&&" || op == "||" => {
            let a = Box::new(native_guard(left, bindings)?);
            let b = Box::new(native_guard(right, bindings)?);
            Ok(if op == "&&" {
                Guard::And(a, b)
            } else {
                Guard::Or(a, b)
            })
        }
        Expr::Call { name, args, .. } => match name.as_str() {
            "Ok" if args.len() == 1 && matches!(args[0], Expr::Unit) => Ok(Guard::Pass),
            "fail" if args.len() == 1 && literal(&args[0]).is_some() => Ok(Guard::Fail),
            "cap_per_transaction" | "set_cap_per_day" => {
                let (limit, token) = if args.len() == 3 && context(&args[0]) {
                    (units(&args[1])?, literal(&args[2]).ok_or_else(unsupported)?)
                } else if name == "cap_per_transaction"
                    && args.len() == 5
                    && field(&args[0], "amount_units")
                    && field(&args[1], "token")
                    && matches!(args[4], Expr::Integer { value: 6 })
                {
                    (units(&args[2])?, literal(&args[3]).ok_or_else(unsupported)?)
                } else if name == "set_cap_per_day"
                    && args.len() == 8
                    && field(&args[0], "daily_spent_units")
                    && field(&args[1], "spent_day")
                    && field(&args[2], "now")
                    && field(&args[3], "amount_units")
                    && field(&args[4], "token")
                    && matches!(args[7], Expr::Integer { value: 6 })
                {
                    (units(&args[5])?, literal(&args[6]).ok_or_else(unsupported)?)
                } else {
                    return Err(unsupported());
                };
                if token != "USDC" || limit <= 0 {
                    return Err(unsupported());
                }
                Ok(if name == "set_cap_per_day" {
                    Guard::Daily(limit, token.into())
                } else {
                    Guard::Transaction(limit, token.into())
                })
            }
            "amount_at_most"
                if args.len() == 2 && (context(&args[0]) || field(&args[0], "amount_units")) =>
            {
                Ok(Guard::Transaction(units(&args[1])?, "USDC".into()))
            }
            "require_recipient"
                if args.len() == 2 && (context(&args[0]) || field(&args[0], "recipient")) =>
            {
                let address = literal(&args[1]).ok_or_else(unsupported)?;
                if address.len() != 56
                    || !matches!(address.as_bytes()[0], b'G' | b'C')
                    || !address
                        .bytes()
                        .all(|c| c.is_ascii_uppercase() || (b'2'..=b'7').contains(&c))
                {
                    return Err(unsupported());
                }
                Ok(Guard::Recipient(address.into()))
            }
            "require_asset"
                if args.len() == 2 && (context(&args[0]) || field(&args[0], "token")) =>
            {
                Ok(Guard::Asset(
                    literal(&args[1]).ok_or_else(unsupported)?.into(),
                ))
            }
            "allow_actions"
                if args.len() == 2 && (context(&args[0]) || field(&args[0], "action")) =>
            {
                let Expr::Array { values } = &args[1] else {
                    return Err(unsupported());
                };
                Ok(Guard::Actions(
                    values
                        .iter()
                        .map(|v| literal(v).map(str::to_owned).ok_or_else(unsupported))
                        .collect::<Result<_, _>>()?,
                ))
            }
            _ => Err(unsupported()),
        },
        _ => Err(unsupported()),
    }
}
fn guard_expr(expr: &Expr, bindings: &BTreeMap<String, Guard>) -> Result<Guard, CompileError> {
    match expr {
        Expr::Try { value } => native_guard(value, bindings),
        _ => native_guard(expr, bindings),
    }
}
fn lower(
    block: &[Statement],
    bindings: &mut BTreeMap<String, Guard>,
) -> Result<Vec<Step>, CompileError> {
    let mut out = Vec::new();
    for statement in block {
        match statement {
            Statement::Let { name, value, .. } => {
                let guard = native_guard(value, bindings)?;
                bindings.insert(name.clone(), guard);
            }
            Statement::Expression {
                value, semicolon, ..
            } => {
                let guard = guard_expr(value, bindings)?;
                out.push(if *semicolon {
                    Step::Check(guard)
                } else {
                    Step::Return(guard)
                });
            }
            Statement::Return { value, .. } => out.push(Step::Return(guard_expr(value, bindings)?)),
            Statement::If {
                condition,
                then_branch,
                else_branch,
                ..
            } => out.push(Step::If(
                native_guard(condition, bindings)?,
                lower(then_branch, &mut bindings.clone())?,
                lower(else_branch, &mut bindings.clone())?,
            )),
            _ => return Err(unsupported()),
        }
    }
    Ok(out)
}
fn check(guard: &Guard, c: &StellarPolicyContext) -> bool {
    match guard {
        Guard::Pass => true,
        Guard::Fail => false,
        Guard::Transaction(limit, token) => {
            c.token == *token && c.decimals == 7 && c.amount <= *limit
        }
        Guard::Daily(limit, token) => {
            c.token == *token
                && c.decimals == 7
                && c.now / 86400 >= c.spent_day
                && if c.now / 86400 == c.spent_day {
                    c.spent_daily
                } else {
                    0
                }
                .checked_add(c.amount)
                .is_some_and(|v| v <= *limit)
        }
        Guard::Recipient(to) => c.recipient == *to,
        Guard::Asset(token) => c.token == *token && c.decimals == 7,
        Guard::Actions(actions) => actions.iter().any(|a| a == "stellar::sep41_transfer"),
        Guard::Not(v) => !check(v, c),
        Guard::And(a, b) => check(a, c) && check(b, c),
        Guard::Or(a, b) => check(a, c) || check(b, c),
    }
}
fn run(block: &[Step], c: &StellarPolicyContext) -> Option<bool> {
    for step in block {
        match step {
            Step::Check(g) => {
                if !check(g, c) {
                    return Some(false);
                }
            }
            Step::Return(g) => return Some(check(g, c)),
            Step::If(g, a, b) => {
                if let Some(value) = run(if check(g, c) { a } else { b }, c) {
                    return Some(value);
                }
            }
        }
    }
    None
}
fn code(guard: &Guard) -> String {
    match guard {
        Guard::Pass => "true".into(),
        Guard::Fail => "false".into(),
        Guard::Transaction(limit, token) => format!(
            "(ctx.token == soroban_sdk::String::from_str(ctx.token.env(), {token:?}) && ctx.decimals == 7 && ctx.amount <= {limit}i128)"
        ),
        Guard::Daily(limit, token) => format!(
            "(ctx.token == soroban_sdk::String::from_str(ctx.token.env(), {token:?}) && ctx.decimals == 7 && ctx.now / 86400 >= ctx.spent_day && (if ctx.now / 86400 == ctx.spent_day {{ ctx.spent_daily }} else {{ 0i128 }}).checked_add(ctx.amount).is_some_and(|next| next <= {limit}i128))"
        ),
        Guard::Recipient(to) => format!(
            "(ctx.recipient == soroban_sdk::Address::from_str(ctx.recipient.env(), {to:?}))"
        ),
        Guard::Asset(token) => format!(
            "(ctx.token == soroban_sdk::String::from_str(ctx.token.env(), {token:?}) && ctx.decimals == 7)"
        ),
        Guard::Actions(a) => a.iter().any(|v| v == "stellar::sep41_transfer").to_string(),
        Guard::Not(v) => format!("!({})", code(v)),
        Guard::And(a, b) => format!("({} && {})", code(a), code(b)),
        Guard::Or(a, b) => format!("({} || {})", code(a), code(b)),
    }
}
fn block_code(steps: &[Step], depth: usize) -> String {
    let mut out = String::new();
    let pad = "    ".repeat(depth);
    for step in steps {
        match step {
            Step::Check(g) => out += &format!("{pad}if !({}) {{ return false; }}\n", code(g)),
            Step::Return(g) => out += &format!("{pad}return {};\n", code(g)),
            Step::If(g, a, b) => {
                out += &format!(
                    "{pad}if {} {{\n{}{pad}}} else {{\n{}{pad}}}\n",
                    code(g),
                    block_code(a, depth + 1),
                    block_code(b, depth + 1)
                );
            }
        }
    }
    out
}
/// Compile the same authored source into a bounded static Stellar policy module.
/// Unsupported source is rejected before an artifact or signing approval is produced.
pub fn compile_stellar_policy(source: &str) -> Result<StellarPolicy, CompileError> {
    let parsed = syn::parse_file(source).map_err(|_| unsupported())?;
    if !parsed.items.iter().any(|i|matches!(i,syn::Item::Fn(f) if f.sig.ident=="_execute"&&matches!(f.vis,syn::Visibility::Inherited))){return Err(unsupported());}
    // Readability helpers lower to the same IR as raw integer comparisons. Admit
    // their explicit monetary meaning while rejecting ambiguous raw source fields.
    struct SourceFields {
        guarded: usize,
        raw: bool,
    }
    impl<'ast> syn::visit::Visit<'ast> for SourceFields {
        fn visit_expr_call(&mut self, node: &'ast syn::ExprCall) {
            let admitted = matches!(&*node.func,syn::Expr::Path(p) if p.path.segments.last().is_some_and(|s|["cap_per_transaction","set_cap_per_day","amount_at_most","require_asset","require_recipient","allow_actions"].contains(&s.ident.to_string().as_str())));
            if admitted {
                self.guarded += 1;
            }
            syn::visit::visit_expr_call(self, node);
            if admitted {
                self.guarded -= 1;
            }
        }
        fn visit_expr_field(&mut self, node: &'ast syn::ExprField) {
            if self.guarded == 0
                && matches!(&*node.base,syn::Expr::Path(p) if p.path.is_ident("ctx"))
            {
                self.raw = true;
            }
            syn::visit::visit_expr_field(self, node);
        }
    }
    let mut fields = SourceFields {
        guarded: 0,
        raw: false,
    };
    syn::visit::Visit::visit_file(&mut fields, &parsed);
    if fields.raw {
        return Err(unsupported());
    }
    let compiled = compile(source)?;
    let steps = lower(&compiled.ir.statements, &mut BTreeMap::new())?;
    let mut mandatory = Vec::new();
    for s in &compiled.ir.statements {
        if let Statement::Expression {
            value: Expr::Try { value },
            ..
        } = s
            && matches!(&**value,Expr::Call{name,..} if name=="set_cap_per_day"||name=="require_asset")
        {
            mandatory.push(native_guard(value, &BTreeMap::new())?);
        }
    }
    let source_hash = digest(source.as_bytes());
    let argument_schema_hash = digest(STELLAR_ARGUMENT_SCHEMA);
    let mut rust_source = format!(
        "// Generated from the exact authored source by allowit-sdk.\nuse allowit_wallet_protocol::PolicyContext;\npub const SOURCE_HASH: [u8; 32] = {source_hash:?};\npub const ARGUMENT_SCHEMA_HASH: [u8; 32] = {argument_schema_hash:?};\npub fn execute(ctx: &PolicyContext) -> bool {{\n    if ctx.amount <= 0 || ctx.spent_daily < 0 || ctx.spent_total < ctx.spent_daily {{ return false; }}\n"
    );
    for guard in &mandatory {
        rust_source += &format!("    if !({}) {{ return false; }}\n", code(guard));
    }
    rust_source += "    _execute(ctx)\n}\n#[allow(unreachable_code, unused_variables)]\nfn _execute(ctx: &PolicyContext) -> bool {\n";
    rust_source += &block_code(&steps, 1);
    rust_source += "    false\n}\n";
    Ok(StellarPolicy {
        source_hash,
        argument_schema_hash,
        rust_source,
        steps,
        mandatory,
    })
}
impl StellarPolicy {
    pub fn evaluate(&self, context: &StellarPolicyContext) -> bool {
        context.amount > 0
            && context.spent_daily >= 0
            && context.spent_total >= context.spent_daily
            && self.mandatory.iter().all(|g| check(g, context))
            && run(&self.steps, context) == Some(true)
    }
}
