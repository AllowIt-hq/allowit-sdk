//! Instruction dependencies from validated, lowered IR. Includes both branches
//! and code after returns: this is conservative coverage, not authorization.
use crate::{CompileError, ExecutionFeature, ExecutionRequirements, Expr, Program, Statement};
use std::collections::BTreeSet;

#[derive(Default)]
struct Collector {
    features: BTreeSet<ExecutionFeature>,
    keys: BTreeSet<String>,
    dynamic: bool,
}

impl Collector {
    fn expr(&mut self, expr: &Expr) -> Result<(), CompileError> {
        match expr {
            Expr::Call { name, args, .. } => {
                let feature = match name.as_str() {
                    "semantic" => Some(ExecutionFeature::SemanticEvidence),
                    "confidence" => Some(ExecutionFeature::ConfidenceEvidence),
                    "require_user_input" => Some(ExecutionFeature::OwnerInput),
                    "cap_purchase_tiers" => Some(ExecutionFeature::PurchaseHistory),
                    "context_u64" => {
                        match args.get(1) {
                            Some(Expr::String { value }) => {
                                self.keys.insert(value.clone());
                            }
                            _ => self.dynamic = true,
                        }
                        Some(ExecutionFeature::RuntimeContextU64)
                    }
                    "set_cap"
                    | "cap_per_transaction"
                    | "allow_actions"
                    | "require_merchant"
                    | "require_recipient"
                    | "fail"
                    | "Ok" => None,
                    // A new primitive must declare its dependencies explicitly.
                    _ => {
                        return Err(CompileError::new(
                            "UNSUPPORTED_REQUIREMENT",
                            format!("No execution requirements declared for {name}."),
                        ));
                    }
                };
                if let Some(feature) = feature {
                    self.features.insert(feature);
                }
                for arg in args {
                    self.expr(arg)?;
                }
            }
            Expr::Try { value } | Expr::Await { value } | Expr::Not { value } => {
                self.expr(value)?
            }
            Expr::Field { object, .. } => self.expr(object)?,
            Expr::Binary { left, right, .. } => {
                self.expr(left)?;
                self.expr(right)?;
            }
            Expr::Array { values } => {
                for value in values {
                    self.expr(value)?;
                }
            }
            Expr::String { .. }
            | Expr::Integer { .. }
            | Expr::Boolean { .. }
            | Expr::Unit
            | Expr::Variable { .. } => {}
        }
        Ok(())
    }

    fn block(&mut self, block: &[Statement]) -> Result<(), CompileError> {
        for statement in block {
            match statement {
                Statement::Let { value, .. }
                | Statement::Return { value, .. }
                | Statement::Expression { value, .. } => self.expr(value)?,
                Statement::If {
                    condition,
                    then_branch,
                    else_branch,
                    ..
                } => {
                    self.expr(condition)?;
                    self.block(then_branch)?;
                    self.block(else_branch)?;
                }
            }
        }
        Ok(())
    }
}

pub(crate) fn extract(program: &Program) -> Result<ExecutionRequirements, CompileError> {
    let mut collector = Collector::default();
    collector.block(&program.statements)?;
    Ok(ExecutionRequirements {
        version: 1,
        features: collector.features.into_iter().collect(),
        context_u64_keys: collector.keys.into_iter().collect(),
        dynamic_context_keys: collector.dynamic,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_primitives_must_declare_dependencies() {
        let mut collector = Collector::default();
        let error = collector
            .expr(&Expr::Call {
                name: "future_primitive".into(),
                args: vec![],
                span: Default::default(),
            })
            .unwrap_err();
        assert_eq!(error.code, "UNSUPPORTED_REQUIREMENT");
    }
}
