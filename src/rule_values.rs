//! Canonical one-argument policy rules. Trusted execution supplies request and accounting context.
use crate::{
    Context, PolicyScalar,
    prelude::{PolicyError, PolicyResult},
};
use alloc::{string::String, vec::Vec};

pub const RULES: &[&str] = &[
    "spent_cap",
    "day_spent_cap",
    "day_total_cap",
    "transaction_cap",
    "call_cap",
    "recipients",
    "expiry",
    "tokens",
];
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GrantRef {
    pub(crate) key: &'static str,
}
impl GrantRef {
    pub const fn key(self) -> &'static str {
        self.key
    }
}
#[allow(non_upper_case_globals)]
pub mod grant {
    use super::GrantRef;
    pub const spent_cap: GrantRef = GrantRef { key: "spent_cap" };
    pub const day_spent_cap: GrantRef = GrantRef {
        key: "day_spent_cap",
    };
    pub const day_total_cap: GrantRef = GrantRef {
        key: "day_total_cap",
    };
    pub const transaction_cap: GrantRef = GrantRef {
        key: "transaction_cap",
    };
    pub const call_cap: GrantRef = GrantRef { key: "call_cap" };
    pub const recipients: GrantRef = GrantRef { key: "recipients" };
    pub const expiry: GrantRef = GrantRef { key: "expiry" };
    pub const tokens: GrantRef = GrantRef { key: "tokens" };
}
#[derive(Clone, Debug)]
pub enum RuleValue {
    Grant(GrantRef),
    Units(u64),
    Addresses(Vec<String>),
    Null,
    Invalid,
}
impl From<GrantRef> for RuleValue {
    fn from(value: GrantRef) -> Self {
        Self::Grant(value)
    }
}
impl From<u64> for RuleValue {
    fn from(value: u64) -> Self {
        Self::Units(value)
    }
}
impl From<i32> for RuleValue {
    fn from(value: i32) -> Self {
        u64::try_from(value)
            .map(Self::Units)
            .unwrap_or(Self::Invalid)
    }
}
impl From<Option<u64>> for RuleValue {
    fn from(value: Option<u64>) -> Self {
        value.map(Self::Units).unwrap_or(Self::Null)
    }
}
impl From<Vec<String>> for RuleValue {
    fn from(value: Vec<String>) -> Self {
        Self::Addresses(value)
    }
}
impl<'a> From<&'a [&'a str]> for RuleValue {
    fn from(value: &'a [&'a str]) -> Self {
        Self::Addresses(value.iter().map(|v| String::from(*v)).collect())
    }
}
impl<'a, const N: usize> From<&'a [&'a str; N]> for RuleValue {
    fn from(value: &'a [&'a str; N]) -> Self {
        Self::from(value.as_slice())
    }
}
fn error(code: &str, reason: &str) -> PolicyError {
    PolicyError {
        code: code.into(),
        reason: reason.into(),
    }
}
macro_rules! facade {($($name:ident),*)=>{$(
    pub fn $name(_value:impl Into<RuleValue>)->PolicyResult{Err(error("POLICY_CONTEXT_REQUIRED","Execute the compiled policy with authenticated request, Grant and Spent context."))}
)*};}
facade!(
    spent_cap,
    day_spent_cap,
    day_total_cap,
    transaction_cap,
    call_cap,
    recipients,
    expiry,
    tokens
);

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct RuleExpense {
    pub spent_units: u64,
    pub gas_units: u64,
}
/// Host-only facts. No public Context JSON can populate this capability.
#[derive(Clone, Debug)]
pub struct PolicyRuleFacts {
    pub operation_id: String,
    /// True only when the host read the owner Grant from native storage.
    pub grant_available: bool,
    pub amount_units: u64,
    pub asset_id: String,
    pub recipient: String,
    pub now: u64,
    pub expense: Option<RuleExpense>,
}
pub(crate) enum Input {
    Grant(String),
    Units(u64),
    Addresses(Vec<String>),
    Null,
}
pub(crate) fn check(ctx: &Context, rule: &str, input: Input) -> PolicyResult {
    if !RULES.contains(&rule) {
        return Err(error(
            "INVALID_POLICY_RULE",
            "Unknown canonical policy rule.",
        ));
    }
    if matches!(&input, Input::Null) {
        return Ok(());
    }
    let state = ctx.policy_state.as_ref().ok_or_else(|| {
        error(
            "POLICY_CONTEXT_REQUIRED",
            "Authenticated owner Grant and Spent are required.",
        )
    })?;
    let value = match input {
        Input::Grant(key) => {
            if !ctx
                .rule_facts
                .as_ref()
                .is_some_and(|facts| facts.grant_available)
            {
                return Err(error(
                    "NATIVE_GRANT_UNAVAILABLE",
                    "This route has no authenticated native Grant map.",
                ));
            }
            state.grant.get(&key).cloned().unwrap_or(PolicyScalar::Null)
        }
        Input::Units(value) => PolicyScalar::U64(value),
        Input::Addresses(value) => PolicyScalar::Strings(value),
        Input::Null => PolicyScalar::Null,
    };
    if value == PolicyScalar::Null {
        return Ok(());
    }
    let facts = ctx.rule_facts.as_ref().ok_or_else(|| {
        error(
            "POLICY_RULE_FACTS_REQUIRED",
            "Authenticated operation facts are required.",
        )
    })?;
    if facts.operation_id.is_empty() || facts.operation_id.len() > 128 {
        return Err(error(
            "INVALID_POLICY_CONTEXT",
            "Operation identity is invalid.",
        ));
    }
    if rule == "recipients" || rule == "tokens" {
        let PolicyScalar::Strings(allowed) = value else {
            return Err(error("INVALID_GRANT", "An address allowlist is required."));
        };
        let actual = if rule == "tokens" {
            &facts.asset_id
        } else {
            &facts.recipient
        };
        return if !actual.is_empty()
            && allowed
                .iter()
                .any(|value| crate::prelude::recipient_matches(actual, value))
        {
            Ok(())
        } else {
            Err(error(
                "ADDRESS_NOT_ALLOWED",
                "The actual native address is not allowed.",
            ))
        };
    }
    let PolicyScalar::U64(limit) = value else {
        return Err(error(
            "INVALID_GRANT",
            "A nonnegative integer limit is required.",
        ));
    };
    if rule == "expiry" {
        return if facts.now < limit {
            Ok(())
        } else {
            Err(error("POLICY_EXPIRED", "The policy has expired."))
        };
    }
    if rule == "transaction_cap" {
        return if facts.amount_units <= limit {
            Ok(())
        } else {
            Err(error(
                "TRANSACTION_CAP_EXCEEDED",
                "The transaction amount exceeds its limit.",
            ))
        };
    }
    let property_values = state.properties.borrow();
    let history_day = match property_values.get("day") {
        Some(PolicyScalar::U64(day)) => *day,
        None => facts.now / 86400,
        _ => {
            return Err(error(
                "INVALID_POLICY_STATE",
                "Historical accounting day has the wrong type.",
            ));
        }
    };
    let unpriced_key = if rule == "spent_cap" {
        Some("unpriced")
    } else if rule == "day_spent_cap" || rule == "day_total_cap" {
        Some("day_unpriced")
    } else {
        None
    };
    if let Some(key) = unpriced_key {
        let unpriced = match property_values.get(key) {
            Some(PolicyScalar::Bool(value)) => *value,
            None => false,
            _ => {
                return Err(error(
                    "INVALID_POLICY_STATE",
                    "Historical accounting status has the wrong type.",
                ));
            }
        };
        if unpriced && (rule == "spent_cap" || history_day == facts.now / 86400) {
            return Err(error(
                "EXPENSE_HISTORY_UNPRICED",
                "The selected cap cannot price prior policy expenses.",
            ));
        }
    }
    drop(property_values);
    let amount = match rule {
        "spent_cap" | "day_spent_cap" => {
            facts
                .expense
                .as_ref()
                .ok_or_else(|| {
                    error(
                        "EXPENSE_BINDING_REQUIRED",
                        "Authenticated operation expenses are required.",
                    )
                })?
                .spent_units
        }
        "day_total_cap" => facts
            .amount_units
            .checked_add(
                facts
                    .expense
                    .as_ref()
                    .ok_or_else(|| {
                        error(
                            "EXPENSE_BINDING_REQUIRED",
                            "Authenticated attributable gas is required.",
                        )
                    })?
                    .gas_units,
            )
            .ok_or_else(|| error("ARITHMETIC_ERROR", "Gross daily amount overflow."))?,
        "call_cap" => 1,
        _ => return Err(error("INVALID_POLICY_RULE", "Unknown counting rule.")),
    };
    let (counter, day_key, op_key) = match rule {
        "spent_cap" => ("spent", None, "spent_operation"),
        "day_spent_cap" => ("day_spent", Some("day_spent_day"), "day_spent_operation"),
        "day_total_cap" => ("day_total", Some("day_total_day"), "day_total_operation"),
        "call_cap" => ("calls", None, "calls_operation"),
        _ => unreachable!(),
    };
    let mut next = state.properties.borrow().clone();
    let integer = |name: &str, default: u64| match next.get(name) {
        None => Ok(default),
        Some(PolicyScalar::U64(v)) => Ok(*v),
        _ => Err(error(
            "INVALID_POLICY_STATE",
            "Stored policy counter has the wrong type.",
        )),
    };
    let day = facts.now / 86400;
    let old_day = day_key
        .map(|key| integer(key, history_day))
        .transpose()?
        .unwrap_or(day);
    if old_day > day {
        return Err(error(
            "INVALID_POLICY_STATE",
            "Policy accounting time moved backwards.",
        ));
    }
    let current = if day_key.is_some() && old_day != day {
        0
    } else {
        integer(counter, 0)?
    };
    let marker = match next.get(op_key) {
        None => None,
        Some(PolicyScalar::String(v)) => Some(v.as_str()),
        _ => {
            return Err(error(
                "INVALID_POLICY_STATE",
                "Stored operation marker has the wrong type.",
            ));
        }
    };
    let fact_key = alloc::format!("{counter}_facts");
    let fact_bytes = serde_json::to_vec(&(
        facts.amount_units,
        &facts.expense,
        &facts.asset_id,
        &facts.recipient,
        day,
    ))
    .map_err(|_| error("INVALID_POLICY_CONTEXT", "Policy facts cannot be encoded."))?;
    let fact_digest = crate::digest(&fact_bytes);
    let repeated = old_day == day && marker == Some(facts.operation_id.as_str());
    if repeated && next.get(&fact_key) != Some(&PolicyScalar::String(fact_digest.clone())) {
        return Err(error(
            "INVALID_POLICY_CONTEXT",
            "Repeated operation accounting facts changed.",
        ));
    }
    let total = if repeated {
        current
    } else {
        current
            .checked_add(amount)
            .ok_or_else(|| error("ARITHMETIC_ERROR", "Policy counter overflow."))?
    };
    if total > limit {
        return Err(error(
            "POLICY_CAP_EXCEEDED",
            "The selected policy limit is exceeded.",
        ));
    }
    next.insert(counter.into(), PolicyScalar::U64(total));
    if let Some(key) = day_key {
        next.insert(key.into(), PolicyScalar::U64(day));
    }
    next.insert(
        op_key.into(),
        PolicyScalar::String(facts.operation_id.clone()),
    );
    next.insert(fact_key, PolicyScalar::String(fact_digest));
    *state.properties.borrow_mut() = next;
    Ok(())
}
