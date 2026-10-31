//! Typed named values supplied by a trusted native adapter.
use crate::prelude::PolicyError;
use alloc::{collections::BTreeMap, string::String};
use core::cell::RefCell;
use serde::{Deserialize, Serialize};

/// Scalar types admitted by the source helpers. Native Grant storage can preserve other values.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ScalarType {
    U64,
    Bool,
    String,
    Unsupported,
}
/// Exact source scalar. Integer values never pass through floating-point numbers.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum PolicyScalar {
    U64(u64),
    Bool(bool),
    String(String),
    /// Host sentinel for a preserved native value outside the typed helper families.
    Unsupported,
}
impl PolicyScalar {
    pub fn kind(&self) -> ScalarType {
        match self {
            Self::U64(_) => ScalarType::U64,
            Self::Bool(_) => ScalarType::Bool,
            Self::String(_) => ScalarType::String,
            Self::Unsupported => ScalarType::Unsupported,
        }
    }
}
/// One author-selected literal key and its expected scalar type.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PolicyKey {
    pub name: String,
    pub kind: ScalarType,
}
/// Source type-checking binding. Native code receives the complete owner map.
#[derive(Clone, Debug, Default)]
pub struct Grant {
    _private: (),
}
/// Owned named execution values. Native bindings preserve all raw native values.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Spent(pub BTreeMap<String, PolicyScalar>);
/// Immutable source state. Replacing spent preserves the Grant binding.
#[derive(Clone, Debug, Default)]
pub struct State {
    grant: Grant,
    spent: Spent,
}
/// Host-only view. Context JSON cannot create this capability.
#[derive(Clone, Debug, Default)]
pub struct TrustedPolicyState {
    pub properties: RefCell<BTreeMap<String, PolicyScalar>>,
    pub grant: BTreeMap<String, PolicyScalar>,
}

/// Type of a source default and its owned result.
pub trait PolicyDefault {
    type Output;
    fn scalar(self) -> PolicyScalar;
    fn decode(value: PolicyScalar) -> Option<Self::Output>;
}
impl PolicyDefault for u64 {
    type Output = u64;
    fn scalar(self) -> PolicyScalar {
        PolicyScalar::U64(self)
    }
    fn decode(v: PolicyScalar) -> Option<u64> {
        if let PolicyScalar::U64(v) = v {
            Some(v)
        } else {
            None
        }
    }
}
impl PolicyDefault for bool {
    type Output = bool;
    fn scalar(self) -> PolicyScalar {
        PolicyScalar::Bool(self)
    }
    fn decode(v: PolicyScalar) -> Option<bool> {
        if let PolicyScalar::Bool(v) = v {
            Some(v)
        } else {
            None
        }
    }
}
impl PolicyDefault for String {
    type Output = String;
    fn scalar(self) -> PolicyScalar {
        PolicyScalar::String(self)
    }
    fn decode(v: PolicyScalar) -> Option<String> {
        if let PolicyScalar::String(v) = v {
            Some(v)
        } else {
            None
        }
    }
}
impl PolicyDefault for &str {
    type Output = String;
    fn scalar(self) -> PolicyScalar {
        PolicyScalar::String(self.into())
    }
    fn decode(v: PolicyScalar) -> Option<String> {
        if let PolicyScalar::String(v) = v {
            Some(v)
        } else {
            None
        }
    }
}
impl From<u64> for PolicyScalar {
    fn from(v: u64) -> Self {
        Self::U64(v)
    }
}
impl From<bool> for PolicyScalar {
    fn from(v: bool) -> Self {
        Self::Bool(v)
    }
}
impl From<String> for PolicyScalar {
    fn from(v: String) -> Self {
        Self::String(v)
    }
}
impl From<&str> for PolicyScalar {
    fn from(v: &str) -> Self {
        Self::String(v.into())
    }
}
fn mismatch() -> PolicyError {
    PolicyError {
        code: "POLICY_STATE_TYPE".into(),
        reason: "The named value has a different scalar type.".into(),
    }
}
/// Copy the current named execution values.
pub fn spent(state: &State) -> Result<Spent, PolicyError> {
    Ok(state.spent.clone())
}
/// Replace the spent map while preserving the same immutable Grant.
pub fn with_spent(state: &State, spent: Spent) -> Result<State, PolicyError> {
    Ok(State {
        grant: state.grant.clone(),
        spent,
    })
}
/// Read `name`; use the source default only when the key does not exist.
pub fn state_get<D: PolicyDefault>(
    spent: &Spent,
    name: &str,
    default: D,
) -> Result<D::Output, PolicyError> {
    let default = default.scalar();
    if default == PolicyScalar::Unsupported {
        return Err(mismatch());
    }
    D::decode(spent.0.get(name).cloned().unwrap_or(default)).ok_or_else(mismatch)
}
/// Read a requested owner value through the generated native Grant binding.
pub fn grant_get<D: PolicyDefault>(
    _grant: &Grant,
    _name: &str,
    _default: D,
) -> Result<D::Output, PolicyError> {
    Err(PolicyError {
        code: "NATIVE_GRANT_REQUIRED".into(),
        reason: "Use the trusted evaluator or generated native Grant binding.".into(),
    })
}
/// Return a new spent map with one typed value replaced. No value is persisted here.
pub fn state_set(
    mut spent: Spent,
    name: &str,
    value: impl Into<PolicyScalar>,
) -> Result<Spent, PolicyError> {
    let value = value.into();
    if value == PolicyScalar::Unsupported {
        return Err(mismatch());
    }
    spent.0.insert(name.into(), value);
    Ok(spent)
}
/// Return a new spent map without `name`; removing an absent name is unchanged.
pub fn state_remove(mut spent: Spent, name: &str) -> Result<Spent, PolicyError> {
    spent.0.remove(name);
    Ok(spent)
}
