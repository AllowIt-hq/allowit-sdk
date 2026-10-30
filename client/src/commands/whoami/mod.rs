//! `allowit whoami`: shows the selected account. Reads `GET /api/gateway/whoami`
//! with `ALLOWIT_ACCOUNT_TOKEN`. Takes no input and refuses `--policy`.
//!
//! ```text
//! export ALLOWIT_URL=https://allowit.example ALLOWIT_ACCOUNT_TOKEN=TOKEN
//! allowit whoami --json
//! allowit skills list
//! ```
use crate::{
    error::{Error, Result},
    gateway::{args::Invocation, control},
};

pub(crate) fn run(inv: &Invocation, stdout: &mut String) -> Result<i32> {
    let c = control::input(inv)?;
    if !c.pos.is_empty() {
        return Err(Error::usage("usage: allowit whoami"));
    }
    control::require_account_authority(inv)?;
    control::public_fields(&c.input, &[])?;
    control::read(inv, "/api/gateway/whoami", None, stdout)
}
