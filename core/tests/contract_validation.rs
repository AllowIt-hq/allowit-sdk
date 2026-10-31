use allowit_sdk::{CompileError, Expr, Program, SourceSpan, Statement, validate_program};

#[test]
fn compact_validation_is_opt_in_and_std_keeps_authoring_diagnostics() {
    let error = CompileError::new("INVALID_POLICY", "Detailed authoring diagnostic.");
    assert_eq!(error.code, "INVALID_POLICY");
    #[cfg(all(feature = "compact-contract-validation", not(feature = "std")))]
    assert!(error.message.is_empty());
    #[cfg(not(all(feature = "compact-contract-validation", not(feature = "std"))))]
    assert_eq!(error.message, "Detailed authoring diagnostic.");
}

#[test]
fn compact_validation_preserves_acceptance_and_rejects_unreachable_bad_calls() {
    let valid = Program {
        version: allowit_sdk::IR_VERSION.into(),
        statements: vec![Statement::Return {
            value: Expr::Call {
                name: "Ok".into(),
                args: vec![Expr::Unit],
                span: SourceSpan::default(),
            },
            span: SourceSpan::default(),
        }],
    };
    validate_program(&valid).unwrap();
    let mut invalid = valid;
    invalid.statements.push(Statement::Expression {
        value: Expr::Call {
            name: "unknown_operation".into(),
            args: vec![],
            span: SourceSpan::default(),
        },
        semicolon: true,
        span: SourceSpan::default(),
    });
    assert_eq!(
        validate_program(&invalid).unwrap_err().code,
        "INVALID_POLICY"
    );
    let decision = allowit_sdk::evaluate_ir(
        &invalid,
        allowit_sdk::Profile::Contract,
        &allowit_sdk::Context::default(),
    );
    assert_eq!(decision.outcome, "fail");
    assert_eq!(decision.code, "INVALID_POLICY");
    #[cfg(all(feature = "compact-contract-validation", not(feature = "std")))]
    assert!(decision.reason.is_empty());
    #[cfg(not(all(feature = "compact-contract-validation", not(feature = "std"))))]
    assert!(!decision.reason.is_empty());
}
