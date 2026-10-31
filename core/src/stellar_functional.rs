//! Functional native-state lowering. Only the map returned by the source is selected.
use crate::{
    CheckPlacement, CompileError, Expr, PolicyKey, PolicyScalar, RulePlacement, ScalarType,
    Statement, StellarCompileOptions,
};
use std::collections::BTreeMap;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    U64,
    Bool,
    String,
    Spent,
    State,
    Grant,
}
impl Kind {
    fn scalar(self) -> Option<ScalarType> {
        match self {
            Self::U64 => Some(ScalarType::U64),
            Self::Bool => Some(ScalarType::Bool),
            Self::String => Some(ScalarType::String),
            _ => None,
        }
    }
}
#[derive(Clone, Debug)]
enum Value {
    Scalar(PolicyScalar),
    Var(String, Kind),
    InitialState,
    Grant,
    Spent(Box<Value>),
    WithSpent(Box<Value>, Box<Value>),
    Read(Box<Value>, String, Box<Value>),
    GrantRead(String, Box<Value>),
    Set(Box<Value>, String, Box<Value>),
    Remove(Box<Value>, String),
    Rule(String, RulePlacement, Box<Value>),
    Binary(String, Box<Value>, Box<Value>),
    Not(Box<Value>),
    Fail,
}
impl Value {
    fn kind(&self) -> Kind {
        match self {
            Self::Scalar(PolicyScalar::U64(_)) => Kind::U64,
            Self::Scalar(PolicyScalar::Bool(_)) => Kind::Bool,
            Self::Scalar(PolicyScalar::String(_)) => Kind::String,
            Self::Scalar(PolicyScalar::Unsupported) => unreachable!(),
            Self::Var(_, kind) => *kind,
            Self::InitialState | Self::WithSpent(..) => Kind::State,
            Self::Grant => Kind::Grant,
            Self::Read(_, _, default) | Self::GrantRead(_, default) => default.kind(),
            Self::Binary(op, _, _) if ["+", "-", "*", "/", "%"].contains(&op.as_str()) => Kind::U64,
            Self::Binary(..) | Self::Not(_) => Kind::Bool,
            Self::Spent(_) | Self::Set(..) | Self::Remove(..) | Self::Rule(..) | Self::Fail => {
                Kind::Spent
            }
        }
    }
}
#[derive(Clone, Debug)]
enum Step {
    Let(String, Value),
    Return(Value),
    If(Value, Vec<Step>, Vec<Step>),
}
#[derive(Clone, Debug)]
pub(crate) struct FunctionalPolicy {
    steps: Vec<Step>,
    pub property_keys: Vec<PolicyKey>,
    pub grant_keys: Vec<PolicyKey>,
    pub placements: Vec<CheckPlacement>,
}
fn bad() -> CompileError {
    CompileError::new(
        "UNSUPPORTED_STELLAR_POLICY",
        "Use typed functional state operations and selected owner-policy rules.",
    )
}
fn literal(expr: &Expr) -> Result<String, CompileError> {
    if let Expr::String { value } = expr {
        Ok(value.clone())
    } else {
        Err(bad())
    }
}
fn method(name: &str) -> Option<&str> {
    let plain = name
        .strip_prefix("engine::")
        .or_else(|| name.strip_prefix("self::"))
        .unwrap_or(name);
    [
        "daily_cap",
        "transaction_cap",
        "total_cap",
        "call_cap",
        "recipient",
        "expiry",
        "token",
    ]
    .contains(&plain)
    .then_some(plain)
}
fn lower_value(
    expr: &Expr,
    env: &BTreeMap<String, Kind>,
    options: &StellarCompileOptions,
    placements: &mut Vec<CheckPlacement>,
) -> Result<Value, CompileError> {
    let value = match expr {
        Expr::Try { value } => return lower_value(value, env, options, placements),
        Expr::Integer { value } => Value::Scalar(PolicyScalar::U64(*value)),
        Expr::Boolean { value } => Value::Scalar(PolicyScalar::Bool(*value)),
        Expr::String { value } => Value::Scalar(PolicyScalar::String(value.clone())),
        Expr::Variable { name } => match name.as_str() {
            "__allowit_state" => Value::InitialState,
            "__allowit_grant" => Value::Grant,
            _ => Value::Var(name.clone(), *env.get(name).ok_or_else(bad)?),
        },
        Expr::Not { value } => Value::Not(Box::new(lower_value(value, env, options, placements)?)),
        Expr::Binary { op, left, right } => Value::Binary(
            op.clone(),
            Box::new(lower_value(left, env, options, placements)?),
            Box::new(lower_value(right, env, options, placements)?),
        ),
        Expr::Call { name, args, .. } => match name.as_str() {
            "__native_return" | "native_ok" => {
                return lower_value(&args[0], env, options, placements);
            }
            "native_fail" => Value::Fail,
            "spent" => Value::Spent(Box::new(lower_value(&args[0], env, options, placements)?)),
            "with_spent" => Value::WithSpent(
                Box::new(lower_value(&args[0], env, options, placements)?),
                Box::new(lower_value(&args[1], env, options, placements)?),
            ),
            "state_get" => Value::Read(
                Box::new(lower_value(&args[0], env, options, placements)?),
                literal(&args[1])?,
                Box::new(lower_value(&args[2], env, options, placements)?),
            ),
            "grant_get" => Value::GrantRead(
                literal(&args[1])?,
                Box::new(lower_value(&args[2], env, options, placements)?),
            ),
            "state_set" => Value::Set(
                Box::new(lower_value(&args[0], env, options, placements)?),
                literal(&args[1])?,
                Box::new(lower_value(&args[2], env, options, placements)?),
            ),
            "state_remove" => Value::Remove(
                Box::new(lower_value(&args[0], env, options, placements)?),
                literal(&args[1])?,
            ),
            name if method(name).is_some() => {
                if args.len() != 2 || !matches!(&args[0],Expr::Variable{name}if name=="ctx") {
                    return Err(bad());
                }
                let rule = method(name).unwrap();
                let requested = if name.starts_with("engine::") {
                    RulePlacement::Shared
                } else if name.starts_with("self::") {
                    RulePlacement::Local
                } else {
                    RulePlacement::Auto
                };
                let plan = crate::stellar_placement::plan(
                    rule,
                    crate::stellar_placement::authority(rule).ok_or_else(bad)?,
                    requested,
                    options,
                )?;
                let selected = plan.selected;
                placements.push(plan);
                Value::Rule(
                    rule.into(),
                    selected,
                    Box::new(lower_value(&args[1], env, options, placements)?),
                )
            }
            _ => return Err(bad()),
        },
        _ => return Err(bad()),
    };
    match &value {
        Value::Not(v) if v.kind() != Kind::Bool => Err(bad()),
        Value::Binary(op, a, b)
            if a.kind() != b.kind()
                || (["+", "-", "*", "/", "%", "<", ">", "<=", ">="].contains(&op.as_str())
                    && a.kind() != Kind::U64)
                || (["&&", "||"].contains(&op.as_str()) && a.kind() != Kind::Bool)
                || a.kind().scalar().is_none() =>
        {
            Err(bad())
        }
        _ => Ok(value),
    }
}
fn lower(
    block: &[Statement],
    env: &mut BTreeMap<String, Kind>,
    options: &StellarCompileOptions,
    placements: &mut Vec<CheckPlacement>,
) -> Result<Vec<Step>, CompileError> {
    let mut out = Vec::new();
    for statement in block {
        out.push(match statement {
            Statement::Let { name, value, .. } => {
                let value = lower_value(value, env, options, placements)?;
                env.insert(name.clone(), value.kind());
                Step::Let(name.clone(), value)
            }
            Statement::Return { value, .. }
            | Statement::Expression {
                value,
                semicolon: false,
                ..
            } => {
                let value = lower_value(value, env, options, placements)?;
                if value.kind() != Kind::Spent {
                    return Err(bad());
                }
                Step::Return(value)
            }
            Statement::If {
                condition,
                then_branch,
                else_branch,
                ..
            } => {
                let condition = lower_value(condition, env, options, placements)?;
                if condition.kind() != Kind::Bool {
                    return Err(bad());
                }
                Step::If(
                    condition,
                    lower(then_branch, &mut env.clone(), options, placements)?,
                    lower(else_branch, &mut env.clone(), options, placements)?,
                )
            }
            _ => return Err(bad()),
        });
    }
    Ok(out)
}
fn collect(value: &Value, properties: &mut Vec<PolicyKey>, grants: &mut Vec<PolicyKey>) {
    match value {
        Value::Read(map, name, default) => {
            let key = PolicyKey {
                name: name.clone(),
                kind: default.kind().scalar().unwrap(),
            };
            if !properties.contains(&key) {
                properties.push(key);
            }
            collect(map, properties, grants);
            collect(default, properties, grants);
        }
        Value::GrantRead(name, default) => {
            let key = PolicyKey {
                name: name.clone(),
                kind: default.kind().scalar().unwrap(),
            };
            if !grants.contains(&key) {
                grants.push(key);
            }
            collect(default, properties, grants);
        }
        Value::WithSpent(a, b) | Value::Binary(_, a, b) => {
            collect(a, properties, grants);
            collect(b, properties, grants);
        }
        Value::Set(map, _, v) => {
            collect(map, properties, grants);
            collect(v, properties, grants);
        }
        Value::Spent(v) | Value::Remove(v, _) | Value::Rule(_, _, v) | Value::Not(v) => {
            collect(v, properties, grants)
        }
        _ => {}
    }
}
fn descriptors(steps: &[Step], properties: &mut Vec<PolicyKey>, grant: &mut Vec<PolicyKey>) {
    for step in steps {
        match step {
            Step::Let(_, v) | Step::Return(v) => collect(v, properties, grant),
            Step::If(v, a, b) => {
                collect(v, properties, grant);
                descriptors(a, properties, grant);
                descriptors(b, properties, grant);
            }
        }
    }
}
fn ty(kind: Kind) -> &'static str {
    match kind {
        Kind::U64 => "u64",
        Kind::Bool => "bool",
        Kind::String => "soroban_sdk::String",
        Kind::Spent => "Spent",
        Kind::State => "EngineState",
        Kind::Grant => "Grant",
    }
}
fn scalar_variant(kind: Kind) -> &'static str {
    match kind {
        Kind::U64 => "U64",
        Kind::Bool => "Bool",
        Kind::String => "String",
        _ => unreachable!(),
    }
}
fn code(value: &Value) -> String {
    match value {
        Value::Scalar(PolicyScalar::U64(v)) => format!("Ok::<u64,()>({v}u64)"),
        Value::Scalar(PolicyScalar::Bool(v)) => format!("Ok::<bool,()>({v})"),
        Value::Scalar(PolicyScalar::String(v)) => format!(
            "Ok::<soroban_sdk::String,()>(soroban_sdk::String::from_str(ctx.asset.env(),{v:?}))"
        ),
        Value::Scalar(PolicyScalar::Unsupported) => unreachable!(),
        Value::Var(name, kind) => format!(
            "Ok::<{},()>({})",
            ty(*kind),
            if matches!(kind, Kind::U64 | Kind::Bool) {
                name.clone()
            } else {
                format!("{name}.clone()")
            }
        ),
        Value::InitialState => "Ok::<EngineState,()>(state.clone())".into(),
        Value::Grant => "Ok::<Grant,()>(grant.clone())".into(),
        Value::Spent(v) => format!("Ok::<Spent,()>(({{ {} }}?).spent)", code(v)),
        Value::WithSpent(old, next) => format!(
            "{{let old={{ {} }}?;let next={{ {} }}?;Ok::<EngineState,()>(EngineState{{grant:old.grant,spent:next}})}}",
            code(old),
            code(next)
        ),
        Value::Read(map, name, default) => format!(
            "{{let map={{ {} }}?;let default={{ {} }}?;match map.get(soroban_sdk::String::from_str(ctx.asset.env(),{name:?})){{Some(value)=>match PolicyValue::from_raw(ctx.asset.env(),&value)?{{PolicyValue::{}(value)=>Ok::<{},()>(value),_=>Err::<{},()>(())}},None=>Ok::<{},()>(default)}}}}",
            code(map),
            code(default),
            scalar_variant(default.kind()),
            ty(default.kind()),
            ty(default.kind()),
            ty(default.kind())
        ),
        Value::GrantRead(name, default) => format!(
            "{{let default={{ {} }}?;match grant.get(soroban_sdk::String::from_str(ctx.asset.env(),{name:?})){{Some(value)=>match PolicyValue::from_raw(ctx.asset.env(),&value)?{{PolicyValue::{}(value)=>Ok::<{},()>(value),_=>Err::<{},()>(())}},None=>Ok::<{},()>(default)}}}}",
            code(default),
            scalar_variant(default.kind()),
            ty(default.kind()),
            ty(default.kind()),
            ty(default.kind())
        ),
        Value::Set(map, name, next) => format!(
            "{{let mut map={{ {} }}?;let value={{ {} }}?;map.set(soroban_sdk::String::from_str(ctx.asset.env(),{name:?}),PolicyValue::{}(value).into_raw(ctx.asset.env()));Ok::<Spent,()>(map)}}",
            code(map),
            code(next),
            scalar_variant(next.kind())
        ),
        Value::Remove(map, name) => format!(
            "{{let mut map={{ {} }}?;map.remove(soroban_sdk::String::from_str(ctx.asset.env(),{name:?}));Ok::<Spent,()>(map)}}",
            code(map)
        ),
        Value::Rule(name, placement, current) => {
            let call = if *placement == RulePlacement::Local {
                format!(
                    "allowit_policy_rules::{name}(ctx.asset.env().clone(),host.request(),current).map_err(|_|())?"
                )
            } else {
                format!(
                    "host.engine_check({name:?},&current.spent,soroban_sdk::Vec::<soroban_sdk::Val>::new(ctx.asset.env()))?"
                )
            };
            format!(
                "{{let current={{ {} }}?;let result={call};if result.approval{{Ok::<Spent,()>(result.spent)}}else{{Err::<Spent,()>(())}}}}",
                code(current)
            )
        }
        Value::Not(v) => format!("Ok::<bool,()>(!({{ {} }}?))", code(v)),
        Value::Binary(op, a, b) => {
            if ["+", "-", "*", "/", "%"].contains(&op.as_str()) {
                format!(
                    "{{let a={{ {} }}?;let b={{ {} }}?;a.{}(b).ok_or(())}}",
                    code(a),
                    code(b),
                    match op.as_str() {
                        "+" => "checked_add",
                        "-" => "checked_sub",
                        "*" => "checked_mul",
                        "/" => "checked_div",
                        "%" => "checked_rem",
                        _ => unreachable!(),
                    }
                )
            } else {
                format!(
                    "Ok::<bool,()>( ({{ {} }}?) {op} ({{ {} }}?) )",
                    code(a),
                    code(b)
                )
            }
        }
        Value::Fail => "Err::<Spent,()>(())".into(),
    }
}
fn block_code(steps: &[Step], depth: usize) -> String {
    let mut result = String::new();
    let pad = "    ".repeat(depth);
    for step in steps {
        match step {
            Step::Let(name, v) => result += &format!("{pad}let {name}={{ {} }}?;\n", code(v)),
            Step::Return(v) => result += &format!("{pad}return {{ {} }};\n", code(v)),
            Step::If(v, a, b) => {
                result += &format!(
                    "{pad}if {{ {} }}? {{\n{}{pad}}}else{{\n{}{pad}}}\n",
                    code(v),
                    block_code(a, depth + 1),
                    block_code(b, depth + 1)
                )
            }
        }
    }
    result
}
#[derive(Clone)]
enum Local {
    Scalar(PolicyScalar),
    Spent(BTreeMap<String, PolicyScalar>),
    State(BTreeMap<String, PolicyScalar>),
    Grant,
}
fn scalar(value: Local) -> Result<PolicyScalar, ()> {
    if let Local::Scalar(v) = value {
        Ok(v)
    } else {
        Err(())
    }
}
fn local(
    value: &Value,
    env: &BTreeMap<String, Local>,
    initial: &BTreeMap<String, PolicyScalar>,
    grant: &BTreeMap<String, PolicyScalar>,
) -> Result<Local, ()> {
    Ok(match value {
        Value::Scalar(v) => Local::Scalar(v.clone()),
        Value::Var(name, _) => env.get(name).cloned().ok_or(())?,
        Value::InitialState => Local::State(initial.clone()),
        Value::Grant => Local::Grant,
        Value::Spent(v) => {
            let Local::State(map) = local(v, env, initial, grant)? else {
                return Err(());
            };
            Local::Spent(map)
        }
        Value::WithSpent(a, b) => {
            let (Local::State(_), Local::Spent(map)) = (
                local(a, env, initial, grant)?,
                local(b, env, initial, grant)?,
            ) else {
                return Err(());
            };
            Local::State(map)
        }
        Value::Read(map, name, default) => {
            let Local::Spent(map) = local(map, env, initial, grant)? else {
                return Err(());
            };
            let default = scalar(local(default, env, initial, grant)?)?;
            let actual = map.get(name).cloned().unwrap_or_else(|| default.clone());
            if actual.kind() != default.kind() {
                return Err(());
            }
            Local::Scalar(actual)
        }
        Value::GrantRead(name, default) => {
            let default = scalar(local(default, env, initial, grant)?)?;
            let actual = grant.get(name).cloned().unwrap_or_else(|| default.clone());
            if actual.kind() != default.kind() {
                return Err(());
            }
            Local::Scalar(actual)
        }
        Value::Set(map, name, next) => {
            let Local::Spent(mut map) = local(map, env, initial, grant)? else {
                return Err(());
            };
            map.insert(name.clone(), scalar(local(next, env, initial, grant)?)?);
            Local::Spent(map)
        }
        Value::Remove(map, name) => {
            let Local::Spent(mut map) = local(map, env, initial, grant)? else {
                return Err(());
            };
            map.remove(name);
            Local::Spent(map)
        }
        Value::Rule(..) | Value::Fail => return Err(()),
        Value::Not(v) => {
            let PolicyScalar::Bool(v) = scalar(local(v, env, initial, grant)?)? else {
                return Err(());
            };
            Local::Scalar(PolicyScalar::Bool(!v))
        }
        Value::Binary(op, a, b) => {
            let a = scalar(local(a, env, initial, grant)?)?;
            if op == "&&" && a == PolicyScalar::Bool(false) {
                return Ok(Local::Scalar(a));
            }
            if op == "||" && a == PolicyScalar::Bool(true) {
                return Ok(Local::Scalar(a));
            }
            let b = scalar(local(b, env, initial, grant)?)?;
            if a.kind() != b.kind() {
                return Err(());
            }
            Local::Scalar(if op == "==" {
                PolicyScalar::Bool(a == b)
            } else if op == "!=" {
                PolicyScalar::Bool(a != b)
            } else {
                match (a, b) {
                    (PolicyScalar::U64(a), PolicyScalar::U64(b)) => match op.as_str() {
                        "<" => PolicyScalar::Bool(a < b),
                        ">" => PolicyScalar::Bool(a > b),
                        "<=" => PolicyScalar::Bool(a <= b),
                        ">=" => PolicyScalar::Bool(a >= b),
                        _ => PolicyScalar::U64(
                            match op.as_str() {
                                "+" => a.checked_add(b),
                                "-" => a.checked_sub(b),
                                "*" => a.checked_mul(b),
                                "/" => a.checked_div(b),
                                "%" => a.checked_rem(b),
                                _ => None,
                            }
                            .ok_or(())?,
                        ),
                    },
                    (PolicyScalar::Bool(a), PolicyScalar::Bool(b)) => {
                        PolicyScalar::Bool(match op.as_str() {
                            "&&" => a && b,
                            "||" => a || b,
                            _ => return Err(()),
                        })
                    }
                    _ => return Err(()),
                }
            })
        }
    })
}
fn run(
    steps: &[Step],
    env: &mut BTreeMap<String, Local>,
    initial: &BTreeMap<String, PolicyScalar>,
    grant: &BTreeMap<String, PolicyScalar>,
) -> Result<Option<BTreeMap<String, PolicyScalar>>, ()> {
    for step in steps {
        match step {
            Step::Let(name, v) => {
                env.insert(name.clone(), local(v, env, initial, grant)?);
            }
            Step::Return(v) => {
                let Local::Spent(map) = local(v, env, initial, grant)? else {
                    return Err(());
                };
                return Ok(Some(map));
            }
            Step::If(v, a, b) => {
                let PolicyScalar::Bool(v) = scalar(local(v, env, initial, grant)?)? else {
                    return Err(());
                };
                if let Some(map) = run(if v { a } else { b }, &mut env.clone(), initial, grant)? {
                    return Ok(Some(map));
                }
            }
        }
    }
    Ok(None)
}
impl FunctionalPolicy {
    /// Recognize the complete fixed-engine program, including state flow and rule order.
    pub(crate) fn common_rules(&self) -> Option<Vec<String>> {
        #[derive(Clone)]
        enum View {
            State(Vec<String>),
            Spent(Vec<String>),
        }
        fn value(
            v: &Value,
            vars: &BTreeMap<String, View>,
            executed: &mut Vec<String>,
        ) -> Option<View> {
            match v {
                Value::InitialState => Some(View::State(Vec::new())),
                Value::Var(name, _) => vars.get(name).cloned(),
                Value::Spent(v) => match value(v, vars, executed)? {
                    View::State(rules) => Some(View::Spent(rules)),
                    _ => None,
                },
                Value::WithSpent(old, next) => {
                    let View::State(_) = value(old, vars, executed)? else {
                        return None;
                    };
                    let View::Spent(rules) = value(next, vars, executed)? else {
                        return None;
                    };
                    Some(View::State(rules))
                }
                Value::Rule(name, RulePlacement::Local, state) => {
                    let View::State(mut rules) = value(state, vars, executed)? else {
                        return None;
                    };
                    if rules != *executed || executed.contains(name) {
                        return None;
                    }
                    rules.push(name.clone());
                    executed.push(name.clone());
                    Some(View::Spent(rules))
                }
                _ => None,
            }
        }
        let mut vars = BTreeMap::new();
        let mut executed = Vec::new();
        for (index, step) in self.steps.iter().enumerate() {
            match step {
                Step::Let(name, v) => {
                    vars.insert(name.clone(), value(v, &vars, &mut executed)?);
                }
                Step::Return(v) if index + 1 == self.steps.len() => {
                    let View::Spent(rules) = value(v, &vars, &mut executed)? else {
                        return None;
                    };
                    return (rules == executed).then_some(rules);
                }
                _ => return None,
            }
        }
        None
    }
    pub fn compile(
        statements: &[Statement],
        options: &StellarCompileOptions,
    ) -> Result<Self, CompileError> {
        let mut placements = Vec::new();
        let steps = lower(statements, &mut BTreeMap::new(), options, &mut placements)?;
        let (mut property_keys, mut grant_keys) = (Vec::new(), Vec::new());
        descriptors(&steps, &mut property_keys, &mut grant_keys);
        Ok(Self {
            steps,
            property_keys,
            grant_keys,
            placements,
        })
    }
    pub fn rust_body(&self) -> String {
        block_code(&self.steps, 1)
    }
    pub fn evaluate(
        &self,
        properties: &mut BTreeMap<String, PolicyScalar>,
        grant: &BTreeMap<String, PolicyScalar>,
    ) -> bool {
        if !self.placements.is_empty() {
            return false;
        }
        match run(&self.steps, &mut BTreeMap::new(), properties, grant) {
            Ok(Some(map)) => {
                *properties = map;
                true
            }
            _ => false,
        }
    }
}
