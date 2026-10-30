//! Help text for the backend command groups, printed by `allowit help` and
//! `allowit COMMAND --help`.
pub(crate) const HELP: &str = "Allowit service commands:\n  allowit call SERVICE_OR_URL [--input JSON] [--policy version@addr] [--sandbox]\n  allowit call status OPERATION_ID\n  allowit curl|wget|http|fetch URL [HTTP options]\n  allowit claude|codex -- [native arguments]\n  allowit whoami\n  allowit setup --input PUBLIC_WALLET_JSON\n  allowit skills [list|search|show] [QUERY]\n  allowit mcp [--policy version@addr] [--sandbox]\n\nCommon flags: --policy version@addr, --sandbox, --account NAME,\n  --request-id ID, --rail RAIL, --rpc URL, --max-payment AMOUNT [--currency USDC], --json.\nPlain calls use the selected account through ALLOWIT_ACCOUNT_TOKEN.\nExplicit --policy uses ALLOWIT_TOKEN and backend policy authorization.\nProduction requires an actively deployed policy revision. Sandbox does not authorize a production RPC.\nA quote is not a payment or a delivery receipt. Agent blockchain keys are generated and retained locally through allowit wallet.\n";

pub(crate) fn command_help(command: &str) -> String {
    let usage = match command {
        "call" => "allowit call SERVICE_OR_URL [--input JSON]\nallowit call status OPERATION_ID",
        "curl" | "wget" | "http" | "fetch" => {
            "allowit COMMAND URL [-X METHOD] [-H 'Name: value'] [-d BODY] [-o FILE]"
        }
        "claude" | "codex" => "allowit COMMAND -- [native arguments]",
        "skills" => "allowit skills [list|search|show] [QUERY]",
        "setup" => "allowit setup --input PUBLIC_WALLET_JSON",
        "mcp" => "allowit mcp [--policy version@addr] [--sandbox]",
        "whoami" => "allowit whoami",
        _ => "allowit COMMAND [--input JSON]",
    };
    format!("{}\n\n{HELP}", usage.replace("COMMAND", command))
}
