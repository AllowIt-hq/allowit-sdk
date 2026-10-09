use crate::{CompileError, Expr, IR_VERSION, MAX_DEPTH, MAX_NODES, Program, Statement};
use alloc::{
    boxed::Box,
    collections::BTreeMap,
    format,
    string::{String, ToString},
    vec::Vec,
};

#[derive(Debug, Clone, PartialEq, Eq)]
enum Type {
    String,
    ContextString,
    Integer,
    Boolean,
    Unit,
    Context,
    Interval,
    Strings,
    Result(Box<Type>),
    Future(Box<Type>),
}
fn result(t: Type) -> Type {
    Type::Result(Box::new(t))
}
fn bad(message: impl Into<String>) -> CompileError {
    CompileError::new("INVALID_POLICY", message)
}

pub(crate) fn amount_units(amount: &str) -> Result<u64, CompileError> {
    if amount.is_empty() || amount.len() > 24 || amount.starts_with('.') || amount.ends_with('.') {
        return Err(bad(
            "Amounts must be positive decimal strings with at most six decimal places.",
        ));
    }
    let mut parts = amount.split('.');
    let whole = parts.next().unwrap_or("");
    let fraction = parts.next().unwrap_or("");
    if parts.next().is_some()
        || fraction.len() > 6
        || !whole.bytes().all(|b| b.is_ascii_digit())
        || !fraction.bytes().all(|b| b.is_ascii_digit())
    {
        return Err(bad(
            "Amounts must use decimal digits and at most six decimal places.",
        ));
    }
    let whole = whole
        .parse::<u64>()
        .map_err(|_| bad("The amount is too large."))?;
    let fraction = if fraction.is_empty() {
        0
    } else {
        fraction
            .parse::<u64>()
            .map_err(|_| bad("Invalid amount."))?
            * 10_u64.pow(6 - fraction.len() as u32)
    };
    let units = whole
        .checked_mul(1_000_000)
        .and_then(|v| v.checked_add(fraction))
        .ok_or_else(|| bad("The amount is too large."))?;
    if units == 0 {
        return Err(bad("Spending limits must be greater than zero."));
    }
    Ok(units)
}

struct Validator {
    nodes: usize,
    config_count: usize,
    provider_call_count: usize,
    provider_profile: bool,
    #[cfg(feature = "oracle-ledger")]
    tier_count: usize,
}
impl Validator {
    fn tick(&mut self, depth: usize) -> Result<(), CompileError> {
        self.nodes += 1;
        if self.nodes > MAX_NODES || depth > MAX_DEPTH {
            Err(bad("Policy complexity exceeds the supported limit."))
        } else {
            Ok(())
        }
    }
    fn block(
        &mut self,
        block: &[Statement],
        env: &mut BTreeMap<String, Type>,
        depth: usize,
        top: bool,
    ) -> Result<bool, CompileError> {
        if block.len() > MAX_NODES {
            return Err(bad("Too many statements."));
        }
        let mut terminal = false;
        for (index, statement) in block.iter().enumerate() {
            self.tick(depth)?;
            match statement {
                Statement::Let {
                    name,
                    value,
                    annotation,
                    ..
                } => {
                    if name == "ctx" || env.contains_key(name) || !valid_name(name) {
                        return Err(bad(
                            "Variables must have distinct names and cannot replace ctx.",
                        ));
                    }
                    let ty = self.expr(value, env, depth + 1, false)?;
                    if !matches!(
                        ty,
                        Type::String
                            | Type::ContextString
                            | Type::Strings
                            | Type::Integer
                            | Type::Boolean
                            | Type::Interval
                    ) {
                        return Err(bad(
                            "A variable must contain an immutable string, integer, boolean or confidence interval.",
                        ));
                    }
                    if let Some(annotation) = annotation {
                        let expected = match annotation.as_str() {
                            "u64" => Type::Integer,
                            "bool" => Type::Boolean,
                            "&str" => Type::String,
                            "ConfidenceInterval" => Type::Interval,
                            _ => return Err(bad("Unsupported variable type.")),
                        };
                        if ty != expected {
                            return Err(bad("Variable type does not match its value."));
                        }
                    }
                    env.insert(name.clone(), ty);
                }
                Statement::Expression {
                    value, semicolon, ..
                } => {
                    let direct_config = top
                        && matches!(value,Expr::Try {value} if matches!(&**value,Expr::Call{name,..} if name=="set_cap" || name=="cap_purchase_tiers"));
                    let ty = self.expr(value, env, depth + 1, direct_config)?;
                    if *semicolon {
                        if ty != Type::Unit {
                            return Err(bad(
                                "Use ? to check every predefined function result. Await user input before propagating its result.",
                            ));
                        }
                    } else {
                        if !top || index + 1 != block.len() || ty != result(Type::Unit) {
                            return Err(bad(
                                "A policy must finish with Ok(()) or return fail(\"reason\").",
                            ));
                        }
                        terminal = true;
                    }
                }
                Statement::Return { value, .. } => {
                    if self.expr(value, env, depth + 1, false)? != result(Type::Unit) {
                        return Err(bad("A return must be Ok(()) or fail(\"reason\")."));
                    }
                    terminal = true;
                }
                Statement::If {
                    condition,
                    then_branch,
                    else_branch,
                    ..
                } => {
                    if self.expr(condition, env, depth + 1, false)? != Type::Boolean {
                        return Err(bad("An if condition must be boolean."));
                    }
                    let a = self.block(then_branch, &mut env.clone(), depth + 1, false)?;
                    let b = self.block(else_branch, &mut env.clone(), depth + 1, false)?;
                    if a && b && !else_branch.is_empty() {
                        terminal = true;
                    }
                }
            }
        }
        Ok(terminal)
    }
    fn expr(
        &mut self,
        expr: &Expr,
        env: &BTreeMap<String, Type>,
        depth: usize,
        config: bool,
    ) -> Result<Type, CompileError> {
        self.tick(depth)?;
        Ok(match expr {
            Expr::String { value } => {
                if value.len() > 1024 {
                    return Err(bad("Strings may contain at most 1,024 bytes."));
                }
                Type::String
            }
            Expr::Integer { .. } => Type::Integer,
            Expr::Boolean { .. } => Type::Boolean,
            Expr::Unit => Type::Unit,
            Expr::Variable { name } => env
                .get(name)
                .cloned()
                .ok_or_else(|| bad(format!("Unknown variable: {name}")))?,
            Expr::Field { object, name } => match self.expr(object, env, depth + 1, false)? {
                Type::Context => match name.as_str() {
                    "amount_units" | "allocation_units" | "spent_units" | "now" => Type::Integer,
                    #[cfg(feature = "std")]
                    "native_daily_limit" | "native_action_limit" => Type::Integer,
                    "action" | "merchant" | "recipient" | "token" | "network" => {
                        Type::ContextString
                    }
                    _ => return Err(bad(format!("Unsupported context field: {name}"))),
                },
                Type::Interval if name == "lower_bps" || name == "upper_bps" => Type::Integer,
                _ => return Err(bad("This value has no supported field with that name.")),
            },
            Expr::Array { values } => {
                if values.is_empty() || values.len() > 32 {
                    return Err(bad("An action list must contain 1 to 32 strings."));
                }
                for value in values {
                    if self.expr(value, env, depth + 1, false)? != Type::String {
                        return Err(bad("Action lists contain only strings."));
                    }
                }
                Type::Strings
            }
            Expr::Binary { op, left, right } => {
                let a = self.expr(left, env, depth + 1, false)?;
                let b = self.expr(right, env, depth + 1, false)?;
                match op.as_str() {
                    "+" | "-" | "*" | "/" | "%" if a == Type::Integer && b == Type::Integer => {
                        Type::Integer
                    }
                    ">" | ">=" | "<" | "<=" if a == Type::Integer && b == Type::Integer => {
                        Type::Boolean
                    }
                    "==" | "!="
                        if a == b && matches!(a, Type::String | Type::Integer | Type::Boolean) =>
                    {
                        Type::Boolean
                    }
                    "==" | "!="
                        if matches!(a, Type::String | Type::ContextString)
                            && matches!(b, Type::String | Type::ContextString) =>
                    {
                        Type::Boolean
                    }
                    "&&" | "||" if a == Type::Boolean && b == Type::Boolean => Type::Boolean,
                    _ => return Err(bad("This operator is not supported for these value types.")),
                }
            }
            Expr::Not { value } => {
                if self.expr(value, env, depth + 1, false)? != Type::Boolean {
                    return Err(bad("Only boolean values support !."));
                }
                Type::Boolean
            }
            Expr::Try { value } => match self.expr(value, env, depth + 1, config)? {
                Type::Result(inner) => *inner,
                _ => return Err(bad("? requires a predefined function result.")),
            },
            Expr::Await { value } => match self.expr(value, env, depth + 1, false)? {
                Type::Future(inner) => *inner,
                _ => return Err(bad("Only require_user_input supports .await.")),
            },
            Expr::Call { name, args, .. } => {
                #[cfg(not(feature = "oracle-ledger"))]
                if name == "cap_purchase_tiers" {
                    return Err(CompileError::new(
                        "LEDGER_REQUIRED",
                        "Purchase tiers require the oracle ledger feature.",
                    ));
                }
                let types = args
                    .iter()
                    .map(|a| self.expr(a, env, depth + 1, false))
                    .collect::<Result<Vec<_>, _>>()?;
                let expected = match name.as_str() {
                    "paysh::call" => alloc::vec![
                        Type::String,
                        Type::String,
                        Type::Integer,
                        Type::Integer,
                        Type::Integer
                    ],
                    "set_cap" | "cap_per_transaction" => {
                        alloc::vec![Type::Context, Type::String, Type::String]
                    }
                    #[cfg(feature = "oracle-ledger")]
                    "cap_purchase_tiers" => {
                        alloc::vec![Type::Context, Type::String, Type::Integer, Type::String]
                    }
                    "allow_actions" => alloc::vec![Type::Context, Type::Strings],
                    "require_merchant" | "require_recipient" | "confidence" | "semantic"
                    | "context_u64" | "require_user_input" => {
                        alloc::vec![Type::Context, Type::String]
                    }
                    "fail" => alloc::vec![Type::String],
                    "Ok" => alloc::vec![Type::Unit],
                    _ => return Err(bad(format!("Unknown function: {name}"))),
                };
                if types != expected {
                    return Err(bad(format!(
                        "Arguments do not match the signature of {name}."
                    )));
                }
                if name == "paysh::call" {
                    if !matches!(args.first(), Some(Expr::String { .. }))
                        || !matches!(args.get(1), Some(Expr::String { .. }))
                        || args[2..]
                            .iter()
                            .any(|arg| !matches!(arg, Expr::Integer { .. }))
                    {
                        return Err(bad(
                            "Provider operation arguments must be source literals or initialized constructor constants.",
                        ));
                    }
                    for arg in &args[..2] {
                        if let Expr::String { value } = arg
                            && (value.trim().is_empty() || value.len() > 128)
                        {
                            return Err(bad("Provider identifiers require 1 to 128 bytes."));
                        }
                    }
                    self.provider_call_count += 1;
                    if self.provider_call_count > 1 {
                        return Err(bad("Declare at most one provider call per policy."));
                    }
                }
                if name == "set_cap" {
                    self.config_count += 1;
                    if !config || self.config_count > 1 {
                        return Err(bad(
                            "set_cap must occur exactly once or not at all, as an unconditional top-level call with literal arguments.",
                        ));
                    }
                    match (args.get(1), args.get(2)) {
                        (
                            Some(Expr::String { value: amount }),
                            Some(Expr::String { value: token }),
                        ) if token == "USDC"
                            || (self.provider_profile
                                && !token.is_empty()
                                && token.len() <= 128) =>
                        {
                            amount_units(amount)?;
                        }
                        _ => {
                            return Err(bad(
                                "set_cap requires a positive amount literal and the token \"USDC\".",
                            ));
                        }
                    }
                }
                #[cfg(feature = "oracle-ledger")]
                if name == "cap_purchase_tiers" {
                    self.tier_count += 1;
                    if self.tier_count > 1 {
                        return Err(bad("Declare purchase tiers only once."));
                    }
                    if !config {
                        return Err(bad(
                            "Purchase tiers must be an unconditional top-level call.",
                        ));
                    }
                    match (args.get(1), args.get(2), args.get(3)) {
                        (
                            Some(Expr::String { value: amount }),
                            Some(Expr::Integer { value: count }),
                            Some(Expr::String { value: token }),
                        ) if (token == "USDC"
                            || (self.provider_profile
                                && !token.is_empty()
                                && token.len() <= 128))
                            && *count > 0
                            && *count <= 1_000_000 =>
                        {
                            if amount_units(amount)? > 1_000_000_000_000 {
                                return Err(bad(
                                    "The purchase ceiling must be at most 1000000 USDC.",
                                ));
                            }
                        }
                        _ => {
                            return Err(bad(
                                "Use a positive maximum USDC amount, a count from 1 to 1000000 and USDC.",
                            ));
                        }
                    }
                }
                if name == "cap_per_transaction" {
                    if let Some(Expr::String { value }) = args.get(1) {
                        amount_units(value)?;
                    }
                    if let Some(Expr::String { value }) = args.get(2)
                        && value != "USDC"
                        && !self.provider_profile
                    {
                        return Err(bad("Version 1 supports six-decimal USDC only."));
                    }
                }
                if [
                    "require_user_input",
                    "require_merchant",
                    "require_recipient",
                    "confidence",
                    "semantic",
                    "context_u64",
                ]
                .contains(&name.as_str())
                    && let Some(Expr::String { value }) = args.get(1)
                    && value.trim().is_empty()
                {
                    return Err(bad("The function argument must not be empty."));
                }
                if name == "paysh::call" {
                    Type::Boolean
                } else if name == "confidence" || name == "semantic" {
                    result(Type::Interval)
                } else if name == "context_u64" {
                    result(Type::Integer)
                } else if name == "require_user_input" {
                    Type::Future(Box::new(result(Type::Unit)))
                } else {
                    result(Type::Unit)
                }
            }
        })
    }
}
fn valid_name(s: &str) -> bool {
    let mut b = s.bytes();
    b.next()
        .is_some_and(|v| v.is_ascii_alphabetic() || v == b'_')
        && b.all(|v| v.is_ascii_alphanumeric() || v == b'_')
        && s.len() <= 64
}

/// Validate every branch of an IR program, including unreachable code.
/// This is also required when a contract receives serialized IR.
pub fn validate_program(program: &Program) -> Result<(), CompileError> {
    if program.version != IR_VERSION {
        return Err(bad("Unsupported canonical IR version."));
    }
    let mut env = BTreeMap::new();
    env.insert("ctx".to_string(), Type::Context);
    let mut validator = Validator {
        provider_call_count: 0,
        provider_profile: provider_call_required(program),
        nodes: 0,
        config_count: 0,
        #[cfg(feature = "oracle-ledger")]
        tier_count: 0,
    };
    if !validator.block(&program.statements, &mut env, 0, true)? {
        return Err(bad(
            "Every policy path must return Ok(()) or fail(\"reason\").",
        ));
    }
    if validator.provider_profile && provider_asset_id(program).is_none() {
        return Err(bad(
            "Provider policies require one unconditional set_cap with a literal payment asset.",
        ));
    }
    Ok(())
}

/// The payment asset comes from the same unconditional numeric guard as funding metadata.
pub(crate) fn provider_asset_id(program: &Program) -> Option<&str> {
    program.statements.iter().find_map(|statement| {
        let Statement::Expression {
            value: Expr::Try { value },
            ..
        } = statement
        else {
            return None;
        };
        let Expr::Call { name, args, .. } = &**value else {
            return None;
        };
        if name != "set_cap" {
            return None;
        }
        match args.get(2) {
            Some(Expr::String { value }) => Some(value.as_str()),
            _ => None,
        }
    })
}

/// Detect native storage reads in every branch, including unreachable statements.
#[cfg(feature = "std")]
pub(crate) fn native_storage_required(program: &Program) -> bool {
    let mut statements: Vec<&Statement> = program.statements.iter().collect();
    let mut expressions = Vec::new();
    while let Some(statement) = statements.pop() {
        match statement {
            Statement::Let { value, .. }
            | Statement::Expression { value, .. }
            | Statement::Return { value, .. } => expressions.push(value),
            Statement::If {
                condition,
                then_branch,
                else_branch,
                ..
            } => {
                expressions.push(condition);
                statements.extend(then_branch);
                statements.extend(else_branch);
            }
        }
    }
    while let Some(expr) = expressions.pop() {
        match expr {
            Expr::Field { object, name } => {
                if name == "native_daily_limit" || name == "native_action_limit" {
                    return true;
                }
                expressions.push(object);
            }
            Expr::Call { args, .. } | Expr::Array { values: args } => expressions.extend(args),
            Expr::Try { value } | Expr::Await { value } | Expr::Not { value } => {
                expressions.push(value)
            }
            Expr::Binary { left, right, .. } => {
                expressions.push(left);
                expressions.push(right);
            }
            Expr::String { .. }
            | Expr::Integer { .. }
            | Expr::Boolean { .. }
            | Expr::Unit
            | Expr::Variable { .. } => {}
        }
    }
    false
}

/// Provider operations require host settlement even when an IR branch is not taken.
pub(crate) fn provider_call_required(program: &Program) -> bool {
    let mut statements: Vec<&Statement> = program.statements.iter().collect();
    let mut expressions = Vec::new();
    while let Some(statement) = statements.pop() {
        match statement {
            Statement::Let { value, .. }
            | Statement::Expression { value, .. }
            | Statement::Return { value, .. } => expressions.push(value),
            Statement::If {
                condition,
                then_branch,
                else_branch,
                ..
            } => {
                expressions.push(condition);
                statements.extend(then_branch);
                statements.extend(else_branch);
            }
        }
    }
    while let Some(expr) = expressions.pop() {
        match expr {
            Expr::Field { object, .. } => {
                expressions.push(object);
            }
            Expr::Call { name, args, .. } => {
                if name == "paysh::call" {
                    return true;
                }
                expressions.extend(args);
            }
            Expr::Array { values } => expressions.extend(values),
            Expr::Try { value } | Expr::Await { value } | Expr::Not { value } => {
                expressions.push(value)
            }
            Expr::Binary { left, right, .. } => {
                expressions.push(left);
                expressions.push(right);
            }
            Expr::String { .. }
            | Expr::Integer { .. }
            | Expr::Boolean { .. }
            | Expr::Unit
            | Expr::Variable { .. } => {}
        }
    }
    false
}
