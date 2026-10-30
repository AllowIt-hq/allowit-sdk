//! `allowit show POLICY`: prints the policy description.
use crate::{
    action::{self, Begin},
    client::Client,
    config::Config,
    error::{Error, Result},
    output,
};

pub(crate) fn run(argv: &[String], stdout: &mut String, stderr: &mut String) -> Result<i32> {
    let parsed = match action::begin("show", argv, stdout, stderr)? {
        Begin::Done(code) => return Ok(code),
        Begin::Policy(parsed) => parsed,
    };
    let policy = &parsed.pos[0];
    let mut c = Client::new(Config::load(policy)?)?;
    let s = action::read_skill(&mut c)?;
    if parsed.json {
        let mut raw = s.0;
        let map = raw
            .as_object_mut()
            .ok_or_else(|| Error::unsupported("Allowit returned an empty policy description"))?;
        map.remove("authorization");
        map.remove("accessUrl");
        if !parsed.source {
            map.remove("policy");
        }
        *stdout += &output::json(&raw);
    } else {
        *stdout += &s.show(policy, parsed.source);
    }
    Ok(0)
}
