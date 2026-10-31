//! The policy compiler commands are local: wallet and network environment
//! must not change their output or exit code.
use std::process::{Command, Output};

const WALLET_ENV: &[&str] = &[
    "ALLOWIT_NETWORK",
    "ALLOWIT_TEMPO_BUNDLE",
    "ALLOWIT_STELLAR_BUNDLE",
    "ALLOWIT_TOKEN",
    "ALLOWIT_ACCOUNT_TOKEN",
    "ALLOWIT_PAYSH_TOKEN",
];

fn allowit(args: &[&str], wallet_env: &[(&str, &str)]) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_allowit"));
    command.args(args);
    for name in WALLET_ENV {
        command.env_remove(name);
    }
    command.envs(wallet_env.iter().copied());
    command.output().unwrap()
}

#[test]
fn compiler_commands_ignore_wallet_environment() {
    let hostile = [
        ("ALLOWIT_NETWORK", "bogus"),
        (
            "ALLOWIT_TEMPO_BUNDLE",
            "/nonexistent/allowit-tempo-bundle.json",
        ),
        (
            "ALLOWIT_STELLAR_BUNDLE",
            "/nonexistent/allowit-stellar-bundle.json",
        ),
        // A token value that occurs in the compiler output.
        ("ALLOWIT_TOKEN", "USDC"),
    ];
    let commands: &[&[&str]] = &[
        &["compile", "../docs/examples/research.rs"],
        &["check", "../docs/examples/research.rs"],
        &["workflow", "../docs/examples/research.rs"],
        &[
            "evaluate",
            "../docs/examples/research.rs",
            "../docs/examples/context.json",
            "contract",
        ],
        &["stellar-native", "../docs/examples/approval.rs"],
        &["registry"],
        &[
            "--network",
            "eip155:1",
            "compile",
            "../docs/examples/research.rs",
        ],
    ];
    for args in commands {
        let clean = allowit(args, &[]);
        let with_env = allowit(args, &hostile);
        assert_eq!(clean.status.code(), with_env.status.code(), "{args:?}");
        assert_eq!(clean.stdout, with_env.stdout, "{args:?}");
        assert_eq!(clean.stderr, with_env.stderr, "{args:?}");
    }
    let compiled = allowit(&["compile", "../docs/examples/research.rs"], &hostile);
    assert!(compiled.status.success());
    assert!(String::from_utf8(compiled.stdout).unwrap().contains("USDC"));
}

#[test]
fn compiler_commands_refuse_leading_global_flags() {
    let cases: &[(&[&str], &str)] = &[
        (
            &[
                "--network",
                "eip155:1",
                "compile",
                "../docs/examples/research.rs",
            ],
            "compile",
        ),
        (
            &[
                "--network=stellar:testnet",
                "check",
                "../docs/examples/research.rs",
            ],
            "check",
        ),
        (&["--json", "registry"], "registry"),
        (
            &[
                "--sandbox",
                "--policy",
                "p",
                "evaluate",
                "../docs/examples/research.rs",
                "../docs/examples/context.json",
            ],
            "evaluate",
        ),
        (&["--json", "json", "-"], "json"),
        (&["--json", "lsp"], "lsp"),
    ];
    for (args, command) in cases {
        let output = allowit(args, &[]);
        assert_eq!(output.status.code(), Some(2), "{args:?}");
        assert!(output.stdout.is_empty(), "{args:?}");
        assert_eq!(
            String::from_utf8(output.stderr).unwrap(),
            format!("allowit: {command} takes no global flags; put the command name first\n"),
            "{args:?}"
        );
    }
    // Flags after the command name stay positional, as in the former
    // standalone compiler binary.
    let output = allowit(&["compile", "--json"], &[]);
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
}
