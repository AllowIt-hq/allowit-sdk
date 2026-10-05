export const RELEASE = {
  "contractRevision": "e0fc5a19985a7c1d1d184754dba98ddbc98e2841",
  "sourceBundle": "c445348186c8d7ee9132539528c82a3c6a718718993db879cd9ceaf28e1d7cfa",
  "artifacts": [
    {
      "name": "allowit_policy.so",
      "bytes": 1696,
      "sha256": "8a7312e81e680a54648c2ad7fc23b77f6cddf70a8c2c733b92076563fd3a19f4"
    },
    {
      "name": "allowit_vault.so",
      "bytes": 100280,
      "sha256": "b7359562591dcef60a43115f7441be00b8205752af3393c4adbb089c8500cb6f"
    }
  ],
  "sources": {
    "policy.rs": "//! Native policy source, copied byte-for-byte into both chain builds.\n//! Amounts use six-decimal policy units. Only daily_limit is explicitly tunable.\nuse crate::policy_api::{Context, PolicyError};\n\npub const MAX_DAILY_LIMIT: u64 = 50_000_000;\npub const DAY_SECONDS: u64 = 86_400;\n\npub fn validate_daily_limit(value: u64) -> Result<(), PolicyError> {\n    if value > MAX_DAILY_LIMIT {\n        return Err(PolicyError::ParameterOutOfBounds);\n    }\n    Ok(())\n}\n\npub fn evaluate(ctx: &Context) -> Result<u64, PolicyError> {\n    if !ctx.approved {\n        return Err(PolicyError::NotApproved);\n    }\n    if ctx.amount == 0 {\n        return Err(PolicyError::ZeroAmount);\n    }\n    validate_daily_limit(ctx.daily_limit)?;\n    let day = ctx.now / DAY_SECONDS;\n    if day < ctx.spent_day {\n        return Err(PolicyError::ClockWentBackwards);\n    }\n    let spent = if day == ctx.spent_day { ctx.spent } else { 0 };\n    let next = spent.checked_add(ctx.amount).ok_or(PolicyError::Overflow)?;\n    if next > ctx.daily_limit {\n        return Err(PolicyError::DailyLimitExceeded);\n    }\n    Ok(next)\n}\n",
    "policy_api.rs": "#[cfg_attr(feature = \"stellar\", soroban_sdk::contracttype)]\n#[derive(Clone, Copy, Debug, PartialEq, Eq)]\npub struct Context {\n    pub approved: bool,\n    pub amount: u64,\n    pub daily_limit: u64,\n    pub spent: u64,\n    pub spent_day: u64,\n    pub now: u64,\n}\n\n#[cfg_attr(feature = \"stellar\", soroban_sdk::contracterror)]\n#[derive(Clone, Copy, Debug, PartialEq, Eq)]\n#[repr(u32)]\npub enum PolicyError {\n    NotApproved = 1,\n    ZeroAmount = 2,\n    ParameterOutOfBounds = 3,\n    ClockWentBackwards = 4,\n    Overflow = 5,\n    DailyLimitExceeded = 6,\n}\n"
  }
};
