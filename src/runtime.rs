#[cfg(feature = "compiler")]
use crate::CompiledPolicy;
use crate::validation::amount_units;
use crate::{
    ConfidenceInterval, Context, Decision, Expr, Profile, Program, Statement, canonical_ir_hash,
    digest, validate_program,
};
use alloc::{
    collections::BTreeMap,
    format,
    string::{String, ToString},
    vec::Vec,
};

#[derive(Debug, Clone, PartialEq, Eq)]
enum Value {
    String(String),
    Integer(u64),
    Boolean(bool),
    Unit,
    Context,
    Interval(ConfidenceInterval),
    Strings(Vec<String>),
}
type EvalResult = Result<Value, alloc::boxed::Box<Decision>>;

struct Evaluator<'a> {
    context: &'a Context,
    profile: Profile,
    binding: String,
    steps: usize,
}
impl Evaluator<'_> {
    fn block(
        &mut self,
        statements: &[Statement],
        env: &mut BTreeMap<String, Value>,
    ) -> Result<bool, alloc::boxed::Box<Decision>> {
        for statement in statements {
            match statement {
                Statement::Let { name, value, .. } => {
                    let value = self.expr(value, env)?;
                    env.insert(name.clone(), value);
                }
                Statement::Expression {
                    value, semicolon, ..
                } => {
                    self.expr(value, env)?;
                    if !semicolon {
                        return Ok(true);
                    }
                }
                Statement::Return { value, .. } => {
                    self.expr(value, env)?;
                    return Ok(true);
                }
                Statement::If {
                    condition,
                    then_branch,
                    else_branch,
                    ..
                } => {
                    let Value::Boolean(condition) = self.expr(condition, env)? else {
                        return Err(invalid());
                    };
                    if self.block(
                        if condition { then_branch } else { else_branch },
                        &mut env.clone(),
                    )? {
                        return Ok(true);
                    }
                }
            }
        }
        Ok(false)
    }
    fn expr(&mut self, expr: &Expr, env: &BTreeMap<String, Value>) -> EvalResult {
        self.steps += 1;
        if self.steps > crate::MAX_NODES * 2 {
            return Err(failure(
                "RESOURCE_LIMIT",
                "Policy evaluation exceeded its operation limit.",
            ));
        }
        Ok(match expr {
            Expr::String { value } => Value::String(value.clone()),
            Expr::Integer { value } => Value::Integer(*value),
            Expr::Boolean { value } => Value::Boolean(*value),
            Expr::Unit => Value::Unit,
            Expr::Variable { name } => env.get(name).cloned().ok_or_else(invalid)?,
            Expr::Field { object, name } => match self.expr(object, env)? {
                Value::Context => match name.as_str() {
                    "amount_units" => Value::Integer(self.context.amount_units),
                    "allocation_units" => Value::Integer(self.context.allocation_units),
                    "spent_units" => Value::Integer(self.context.spent_units),
                    "now" => Value::Integer(self.context.now),
                    "action" => Value::String(self.context.action.clone()),
                    "merchant" => Value::String(self.context.merchant.clone()),
                    "recipient" => Value::String(self.context.recipient.clone()),
                    "token" => Value::String(self.context.token.clone()),
                    "network" => Value::String(self.context.network.clone()),
                    _ => return Err(invalid()),
                },
                Value::Interval(interval) => match name.as_str() {
                    "lower_bps" => Value::Integer(interval.lower_bps),
                    "upper_bps" => Value::Integer(interval.upper_bps),
                    _ => return Err(invalid()),
                },
                _ => return Err(invalid()),
            },
            Expr::Array { values } => Value::Strings(
                values
                    .iter()
                    .map(|v| match self.expr(v, env)? {
                        Value::String(s) => Ok(s),
                        _ => Err(invalid()),
                    })
                    .collect::<Result<Vec<_>, _>>()?,
            ),
            Expr::Not { value } => match self.expr(value, env)? {
                Value::Boolean(v) => Value::Boolean(!v),
                _ => return Err(invalid()),
            },
            Expr::Try { value } | Expr::Await { value } => self.expr(value, env)?,
            Expr::Binary { op, left, right } => {
                let a = self.expr(left, env)?;
                if op == "&&" && a == Value::Boolean(false) {
                    return Ok(Value::Boolean(false));
                }
                if op == "||" && a == Value::Boolean(true) {
                    return Ok(Value::Boolean(true));
                }
                let b = self.expr(right, env)?;
                match op.as_str() {
                    "==" => Value::Boolean(a == b),
                    "!=" => Value::Boolean(a != b),
                    "&&" | "||" => match (a, b) {
                        (Value::Boolean(a), Value::Boolean(b)) => {
                            Value::Boolean(if op == "&&" { a && b } else { a || b })
                        }
                        _ => return Err(invalid()),
                    },
                    _ => {
                        let (Value::Integer(a), Value::Integer(b)) = (a, b) else {
                            return Err(invalid());
                        };
                        match op.as_str(){
                            ">"=>Value::Boolean(a>b),">="=>Value::Boolean(a>=b),"<"=>Value::Boolean(a<b),"<="=>Value::Boolean(a<=b),
                            "+"|"-"|"*"|"/"|"%"=>Value::Integer(match op.as_str(){"+"=>a.checked_add(b),"-"=>a.checked_sub(b),"*"=>a.checked_mul(b),"/"=>a.checked_div(b),"%"=>a.checked_rem(b),_=>None}.ok_or_else(||failure("ARITHMETIC_ERROR","A policy calculation overflowed, underflowed or divided by zero."))?),
                            _=>return Err(invalid()),
                        }
                    }
                }
            }
            Expr::Call { name, args, span } => {
                let values = args
                    .iter()
                    .map(|a| self.expr(a, env))
                    .collect::<Result<Vec<_>, _>>()?;
                match name.as_str() {
                    "Ok" => Value::Unit,
                    "fail" => return Err(failure("POLICY_REJECTED", string(&values, 0)?)),
                    "set_cap" | "cap_per_transaction" => {
                        let token = string(&values, 2)?;
                        if token != "USDC" || token != self.context.token {
                            return Err(failure(
                                "TOKEN_MISMATCH",
                                "The requested token does not match the policy.",
                            ));
                        }
                        let cap = amount_units(&string(&values, 1)?)
                            .map_err(|e| failure("INVALID_AMOUNT", e.message))?;
                        let amount = if name == "set_cap" {
                            self.context
                                .spent_units
                                .checked_add(self.context.amount_units)
                                .ok_or_else(|| {
                                    failure(
                                        "BUDGET_EXCEEDED",
                                        "The request exceeds the remaining allowance.",
                                    )
                                })?
                        } else {
                            self.context.amount_units
                        };
                        if amount > cap {
                            return Err(failure(
                                if name == "set_cap" {
                                    "POLICY_CAP_EXCEEDED"
                                } else {
                                    "PURCHASE_CAP_EXCEEDED"
                                },
                                if name == "set_cap" {
                                    "The request exceeds this policy's remaining spending limit."
                                } else {
                                    "The purchase exceeds the per-transaction limit."
                                },
                            ));
                        }
                        Value::Unit
                    }
                    "allow_actions" => {
                        let Some(Value::Strings(actions)) = values.get(1) else {
                            return Err(invalid());
                        };
                        if !actions.contains(&self.context.action) {
                            return Err(failure(
                                "ACTION_NOT_ALLOWED",
                                "This action is not permitted by the policy.",
                            ));
                        }
                        Value::Unit
                    }
                    "require_merchant" => {
                        if string(&values, 1)? != self.context.merchant {
                            return Err(failure(
                                "MERCHANT_NOT_ALLOWED",
                                "This merchant is not permitted by the policy.",
                            ));
                        }
                        Value::Unit
                    }
                    "require_recipient" => {
                        if string(&values, 1)? != self.context.recipient {
                            return Err(failure(
                                "RECIPIENT_NOT_ALLOWED",
                                "This recipient is not permitted by the policy.",
                            ));
                        }
                        Value::Unit
                    }
                    "context_u64" => {
                        let key = string(&values, 1)?;
                        let value = self.context.runtime_context.get(&key).ok_or_else(|| {
                            failure(
                                "CONTEXT_VALUE_REQUIRED",
                                format!("The request must supply {key}."),
                            )
                        })?;
                        Value::Integer(value.as_u64().ok_or_else(|| {
                            failure(
                                "INVALID_CONTEXT_VALUE",
                                format!(
                                    "{key} must be a non-negative whole number that fits in u64."
                                ),
                            )
                        })?)
                    }
                    "confidence" | "semantic" => {
                        let is_semantic = name == "semantic";
                        let label = string(&values, 1)?;
                        if label.trim().is_empty() || label.len() > 1024 {
                            return Err(failure(
                                "INVALID_EVIDENCE",
                                "The assessment question is empty or exceeds its size limit.",
                            ));
                        }
                        if is_semantic && self.context.original_intent.trim().is_empty() {
                            return Err(failure(
                                "ORIGINAL_INTENT_REQUIRED",
                                "The original policy instructions are required for a preference assessment.",
                            ));
                        }
                        let key = if is_semantic {
                            crate::semantic_evidence_key(&label)
                        } else {
                            label.clone()
                        };
                        let interval = self.context.confidence.get(&key).ok_or_else(|| {
                            let mut decision = failure(
                                if is_semantic {
                                    "SEMANTIC_EVIDENCE_REQUIRED"
                                } else {
                                    "EVIDENCE_REQUIRED"
                                },
                                if is_semantic {
                                    "A preference assessment is required for this request.".into()
                                } else {
                                    format!("Confidence evidence is required for {key}.")
                                },
                            );
                            if is_semantic {
                                decision.question = Some(label);
                                decision.evidence_key = Some(key);
                            }
                            decision
                        })?;
                        if interval.lower_bps > interval.upper_bps || interval.upper_bps > 10000 {
                            return Err(failure(
                                "INVALID_EVIDENCE",
                                "Confidence bounds must be ordered between 0 and 10,000 basis points.",
                            ));
                        }
                        Value::Interval(*interval)
                    }
                    "require_user_input" => {
                        let prompt = string(&values, 1)?;
                        if prompt.trim().is_empty() {
                            return Err(failure(
                                "INVALID_INPUT_PROMPT",
                                "The approval prompt is empty.",
                            ));
                        }
                        if self.profile == Profile::Contract {
                            return Err(failure(
                                "USER_INPUT_REQUIRED",
                                "This request requires user input and cannot execute in a smart contract.",
                            ));
                        }
                        let key = digest(
                            format!(
                                "allowit-input-v1:{}:{}:{}",
                                self.binding, span.start, prompt
                            )
                            .as_bytes(),
                        );
                        match self.context.answers.get(&key) {
                            Some(true) => Value::Unit,
                            Some(false) => {
                                return Err(failure(
                                    "USER_DECLINED",
                                    "The policy owner declined this request.",
                                ));
                            }
                            None => {
                                return Err(alloc::boxed::Box::new(Decision {
                                    outcome: "awaiting_input".into(),
                                    code: "USER_INPUT_REQUIRED".into(),
                                    reason:
                                        "Your approval is required before evaluation can continue."
                                            .into(),
                                    prompt: Some(prompt),
                                    input_key: Some(key),
                                    question: None,
                                    evidence_key: None,
                                }));
                            }
                        }
                    }
                    _ => return Err(invalid()),
                }
            }
        })
    }
}
fn string(values: &[Value], index: usize) -> Result<String, alloc::boxed::Box<Decision>> {
    match values.get(index) {
        Some(Value::String(s)) => Ok(s.clone()),
        _ => Err(invalid()),
    }
}
fn invalid() -> alloc::boxed::Box<Decision> {
    failure(
        "INVALID_POLICY",
        "The policy contains an invalid operation.",
    )
}

fn validate_context(ctx: &Context) -> Result<(), alloc::boxed::Box<Decision>> {
    if ctx.token != "USDC" {
        return Err(failure(
            "TOKEN_MISMATCH",
            "This policy supports six-decimal USDC.",
        ));
    }
    if ![
        "mainnet",
        "mainnet-beta",
        "devnet",
        "testnet",
        "stellar-mainnet",
        "stellar-testnet",
        "solana:mainnet",
        "solana:devnet",
        "solana:testnet",
        "stellar:mainnet",
        "stellar:testnet",
    ]
    .contains(&ctx.network.as_str())
    {
        return Err(failure(
            "INVALID_NETWORK",
            "The requested network is not supported.",
        ));
    }
    if ctx.amount_units == 0 || ctx.allocation_units == 0 {
        return Err(failure(
            "INVALID_AMOUNT",
            "The request and allocation amounts must be greater than zero.",
        ));
    }
    if ctx
        .spent_units
        .checked_add(ctx.amount_units)
        .is_none_or(|n| n > ctx.allocation_units)
    {
        return Err(failure(
            "BUDGET_EXCEEDED",
            "The request exceeds the remaining allowance.",
        ));
    }
    if ctx.action.is_empty()
        || ctx.action.len() > 128
        || ctx.merchant.len() > 256
        || ctx.recipient.len() > 256
        || ctx.answers.len() > 32
        || ctx.confidence.len() > 32
        || ctx.answers.keys().any(|s| s.len() != 64)
        || ctx.confidence.keys().any(|s| s.is_empty() || s.len() > 128)
        || ctx.original_intent.len() > 16384
    {
        return Err(failure(
            "INVALID_CONTEXT",
            "The request context is missing required values or exceeds its size limit.",
        ));
    }
    validate_runtime_context(&ctx.runtime_context)?;
    if ctx
        .confidence
        .values()
        .any(|v| v.lower_bps > v.upper_bps || v.upper_bps > 10000)
    {
        return Err(failure(
            "INVALID_EVIDENCE",
            "Confidence bounds must be ordered between 0 and 10,000 basis points.",
        ));
    }
    Ok(())
}

fn validate_runtime_context(value: &serde_json::Value) -> Result<(), alloc::boxed::Box<Decision>> {
    use serde_json::Value;
    let invalid = || {
        failure(
            "INVALID_CONTEXT",
            "Runtime context must be an object of at most 16 KiB, depth 8 and 128 entries.",
        )
    };
    if !value.is_object() {
        return Err(invalid());
    }
    let mut stack = alloc::vec![(value, 0usize)];
    let mut entries = 0usize;
    while let Some((value, depth)) = stack.pop() {
        if depth > 8 {
            return Err(invalid());
        }
        match value {
            Value::Object(map) => {
                entries += map.len();
                if entries > 128 || map.keys().any(|key| key.is_empty() || key.len() > 128) {
                    return Err(invalid());
                }
                for child in map.values() {
                    stack.push((child, depth + 1));
                }
            }
            Value::Array(values) => {
                entries += values.len();
                if entries > 128 {
                    return Err(invalid());
                }
                for child in values {
                    stack.push((child, depth + 1));
                }
            }
            Value::String(value) if value.len() > 16384 => return Err(invalid()),
            _ => {}
        }
    }
    if serde_json::to_vec(value).map_err(|_| invalid())?.len() > 16384 {
        return Err(invalid());
    }
    Ok(())
}

fn failure(code: &str, reason: impl Into<String>) -> alloc::boxed::Box<Decision> {
    alloc::boxed::Box::new(Decision::fail(code, reason))
}

fn run(ir: &Program, profile: Profile, ctx: &Context, binding: String) -> Decision {
    if let Err(error) = validate_program(ir) {
        return Decision::fail("INVALID_POLICY", error.message);
    }
    if let Err(error) = validate_context(ctx) {
        return *error;
    }
    let mut evaluator = Evaluator {
        context: ctx,
        profile,
        binding,
        steps: 0,
    };
    let mut env = BTreeMap::new();
    env.insert("ctx".to_string(), Value::Context);
    // Configuration is enforced before control flow, so even an early return cannot bypass the cap.
    for statement in &ir.statements {
        if let Statement::Expression {
            value: Expr::Try { value },
            ..
        } = statement
            && matches!(&**value,Expr::Call{name,..} if name=="set_cap")
            && let Err(decision) = evaluator.expr(value, &env)
        {
            return *decision;
        }
    }
    match evaluator.block(&ir.statements, &mut env) {
        Ok(true) => Decision::pass(),
        Ok(false) => *invalid(),
        Err(decision) => *decision,
    }
}

/// Evaluate an already compiled policy. Persisted artifacts are hash-checked before evaluation.
#[cfg(feature = "compiler")]
pub fn evaluate(policy: &CompiledPolicy, profile: Profile, ctx: &Context) -> Decision {
    if policy.language != crate::LANGUAGE
        || policy.registry_version != crate::REGISTRY_VERSION
        || digest(policy.source.as_bytes()) != policy.source_hash
        || canonical_ir_hash(&policy.ir).ok().as_ref() != Some(&policy.ir_hash)
    {
        return Decision::fail(
            "INVALID_ARTIFACT",
            "The policy artifact failed its integrity check.",
        );
    }
    #[cfg(feature = "compiler")]
    {
        let compiled = match crate::compile(&policy.source) {
            Ok(value) => value,
            Err(_) => return Decision::fail("INVALID_ARTIFACT", "The policy source is not valid."),
        };
        if compiled.ir_hash != policy.ir_hash
            || compiled.limit != policy.limit
            || compiled.token != policy.token
        {
            return Decision::fail(
                "INVALID_ARTIFACT",
                "The policy source does not match its executable artifact.",
            );
        }
    }
    run(&policy.ir, profile, ctx, policy.source_hash.clone())
}

/// Evaluate validated IR inside a target adapter. The adapter must authenticate its owner and
/// bind the canonical IR hash, network, asset, action and fresh budget to the mandate.
pub fn evaluate_ir(ir: &Program, profile: Profile, ctx: &Context) -> Decision {
    let binding = match canonical_ir_hash(ir) {
        Ok(hash) => hash,
        Err(error) => return Decision::fail("INVALID_POLICY", error.message),
    };
    run(ir, profile, ctx, binding)
}
