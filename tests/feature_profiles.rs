use allowit_sdk::{Expr, Program, REGISTRY_VERSION, SourceSpan, Statement, validate_program};
#[test]
fn purchase_tiers_require_the_ledger_feature_before_evaluation() {
    let program = Program {
        version: REGISTRY_VERSION.into(),
        statements: vec![
            Statement::Expression {
                span: SourceSpan::default(),
                semicolon: true,
                value: Expr::Try {
                    value: Box::new(Expr::Call {
                        name: "cap_purchase_tiers".into(),
                        args: vec![
                            Expr::Variable { name: "ctx".into() },
                            Expr::String { value: "1".into() },
                            Expr::Integer { value: 2 },
                            Expr::String {
                                value: "USDC".into(),
                            },
                        ],
                        span: SourceSpan::default(),
                    }),
                },
            },
            Statement::Expression {
                span: SourceSpan::default(),
                semicolon: false,
                value: Expr::Call {
                    name: "Ok".into(),
                    args: vec![Expr::Unit],
                    span: SourceSpan::default(),
                },
            },
        ],
    };
    #[cfg(feature = "oracle-ledger")]
    assert!(validate_program(&program).is_ok());
    #[cfg(not(feature = "oracle-ledger"))]
    assert_eq!(
        validate_program(&program).unwrap_err().code,
        "LEDGER_REQUIRED"
    );
    assert!(
        allowit_sdk::registry()
            .iter()
            .any(|f| f.name == "cap_purchase_tiers")
    );
}
