use alloc::{collections::BTreeMap, string::String, vec::Vec};
use serde::{Deserialize, Serialize};

pub const LANGUAGE: &str = "allowit-rust-v1";
/// Source registry version. Explicit namespaces are available from 1.1.0.
pub const REGISTRY_VERSION: &str = "1.2.0";
/// Canonical operation schema. Source-only aliases do not change this version.
pub const IR_VERSION: &str = "1.0.0";
/// Both registries use the same canonical operations and artifact semantics.
pub fn supported_registry_version(version: &str) -> bool {
    matches!(version, "1.0.0" | "1.1.0" | REGISTRY_VERSION)
}
pub const MAX_SOURCE_BYTES: usize = 32768;
pub const MAX_NODES: usize = 2048;
pub const MAX_DEPTH: usize = 48;
pub const TOKEN_DECIMALS: u32 = 6;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompileError {
    pub code: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub column: Option<usize>,
}
impl CompileError {
    pub fn new(code: &str, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            line: None,
            column: None,
        }
    }
}
impl core::fmt::Display for CompileError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(&self.message)
    }
}
#[cfg(feature = "std")]
impl std::error::Error for CompileError {}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceSpan {
    pub start: usize,
    pub end: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Expr {
    String {
        value: String,
    },
    Integer {
        value: u64,
    },
    Boolean {
        value: bool,
    },
    Unit,
    Variable {
        name: String,
    },
    Field {
        object: alloc::boxed::Box<Expr>,
        name: String,
    },
    Array {
        values: Vec<Expr>,
    },
    Binary {
        op: String,
        left: alloc::boxed::Box<Expr>,
        right: alloc::boxed::Box<Expr>,
    },
    Not {
        value: alloc::boxed::Box<Expr>,
    },
    Call {
        name: String,
        args: Vec<Expr>,
        span: SourceSpan,
    },
    Try {
        value: alloc::boxed::Box<Expr>,
    },
    Await {
        value: alloc::boxed::Box<Expr>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Statement {
    Let {
        name: String,
        value: Expr,
        annotation: Option<String>,
        span: SourceSpan,
    },
    Expression {
        value: Expr,
        semicolon: bool,
        span: SourceSpan,
    },
    Return {
        value: Expr,
        span: SourceSpan,
    },
    If {
        condition: Expr,
        then_branch: Vec<Statement>,
        else_branch: Vec<Statement>,
        span: SourceSpan,
    },
}
impl Statement {
    pub fn span(&self) -> SourceSpan {
        match self {
            Self::Let { span, .. }
            | Self::Expression { span, .. }
            | Self::Return { span, .. }
            | Self::If { span, .. } => *span,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Program {
    pub version: String,
    pub statements: Vec<Statement>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkflowBlock {
    pub id: String,
    pub kind: String,
    pub name: String,
    pub label: String,
    pub description: String,
    pub arguments: Vec<String>,
    pub source: String,
    /// UTF-16 code-unit offsets, compatible with browser strings and LSP.
    pub start: usize,
    pub end: usize,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub score_thresholds: Vec<ScoreThreshold>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScoreThreshold {
    pub field: String,
    pub operator: String,
    pub value_bps: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CallSite {
    pub name: String,
    pub start: usize,
    pub end: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompiledPolicy {
    pub language: String,
    pub source_hash: String,
    pub ir_hash: String,
    pub registry_version: String,
    pub execution_requirements: ExecutionRequirements,
    pub limit: String,
    pub token: String,
    pub source: String,
    pub workflow: Vec<WorkflowBlock>,
    pub calls: Vec<CallSite>,
    pub ir: Program,
}

/// Conservative dependencies of all IR paths, not permissions or reachability.
/// Bound to the enclosing compiled policy's source_hash and ir_hash.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExecutionRequirements {
    pub version: u32,
    pub features: Vec<ExecutionFeature>,
    pub context_u64_keys: Vec<String>,
    /// A nonliteral context key exists; the consumer must not invent its value.
    pub dynamic_context_keys: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionFeature {
    NativePolicyStorage,
    ConfidenceEvidence,
    OwnerInput,
    PurchaseHistory,
    RuntimeContextU64,
    SemanticEvidence,
}

/// Oracle-only execution evidence for the compiler's source-bound workflow.
/// A custom block reports the path actually taken, not every nested branch.
#[cfg(feature = "compiler")]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkflowTrace {
    pub version: u8,
    pub source_hash: String,
    pub ir_hash: String,
    /// False while evaluation requires semantic evidence or an owner answer.
    pub complete: bool,
    pub steps: Vec<WorkflowStepTrace>,
}

#[cfg(feature = "compiler")]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkflowStepTrace {
    pub node_id: String,
    pub status: WorkflowStepStatus,
    /// Includes configuration checks performed before ordinary control flow.
    pub visited: bool,
}

#[cfg(feature = "compiler")]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowStepStatus {
    Passed,
    Failed,
    Verifying,
    AwaitingInput,
    Inactive,
    Skipped,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Profile {
    #[default]
    Oracle,
    Contract,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConfidenceInterval {
    pub lower_bps: u64,
    pub upper_bps: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NativePolicyStorage {
    pub daily_limit_units: u64,
    pub action_limit_units: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Context {
    /// Trusted current native account state. Adapters must overwrite caller-supplied values.
    #[serde(default)]
    pub native_policy_storage: Option<NativePolicyStorage>,
    pub amount_units: u64,
    pub allocation_units: u64,
    pub spent_units: u64,
    /// Trusted host counts for the single declared geometric purchase-tier rule.
    #[serde(default)]
    pub purchase_counts: Option<Vec<u64>>,
    pub action: String,
    pub merchant: String,
    #[serde(default)]
    pub recipient: String,
    pub token: String,
    pub network: String,
    pub now: u64,
    #[serde(default)]
    pub original_intent: String,
    #[serde(default = "empty_runtime_context")]
    pub runtime_context: serde_json::Value,
    #[serde(default)]
    pub answers: BTreeMap<String, bool>,
    #[serde(default)]
    pub confidence: BTreeMap<String, ConfidenceInterval>,
}
fn empty_runtime_context() -> serde_json::Value {
    serde_json::Value::Object(serde_json::Map::new())
}
impl Default for Context {
    fn default() -> Self {
        Self {
            native_policy_storage: None,
            amount_units: 0,
            allocation_units: 0,
            spent_units: 0,
            purchase_counts: None,
            action: String::new(),
            merchant: String::new(),
            recipient: String::new(),
            token: String::new(),
            network: String::new(),
            now: 0,
            original_intent: String::new(),
            runtime_context: empty_runtime_context(),
            answers: BTreeMap::new(),
            confidence: BTreeMap::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Decision {
    pub outcome: String,
    pub code: String,
    pub reason: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input_key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub question: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub evidence_key: Option<String>,
    /// UTF-8 byte offsets of the operation that halted evaluation, when known.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_start: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_end: Option<usize>,
}
impl Decision {
    pub fn fail(code: &str, reason: impl Into<String>) -> Self {
        Self {
            outcome: "fail".into(),
            code: code.into(),
            reason: reason.into(),
            prompt: None,
            input_key: None,
            question: None,
            evidence_key: None,
            source_start: None,
            source_end: None,
        }
    }
    pub fn pass() -> Self {
        Self {
            outcome: "pass".into(),
            code: "PASS".into(),
            reason: "The request satisfies this policy.".into(),
            prompt: None,
            input_key: None,
            question: None,
            evidence_key: None,
            source_start: None,
            source_end: None,
        }
    }
}
