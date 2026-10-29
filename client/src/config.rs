use crate::error::{Error, Result, quoted};
use url::Url;
#[derive(Clone)]
pub(crate) struct Config {
    pub origin: String,
    pub owner: String,
    pub policy: String,
    pub token: String,
    pub ca_file: String,
}
fn valid_id(s: &str, min: usize, max: usize) -> bool {
    s.len() >= min
        && s.len() <= max
        && s.bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'_' || c == b'-')
}
pub(crate) use allowit_client_core::config::parse_origin;
pub(crate) fn env_value(name: &str) -> String {
    if name == "ALLOWIT_NETWORK"
        && let Some(value) = crate::routing::selected()
    {
        return value;
    }
    allowit_client_core::config::env_value(name)
}
impl Config {
    pub fn load(policy: &str) -> Result<Self> {
        let origin = parse_origin(&env_value("ALLOWIT_URL"))?;
        let raw = env_value("ALLOWIT_TOKEN");
        if raw.is_empty() {
            return Err(Error::config(
                "set ALLOWIT_TOKEN to the policy's harness token",
            ));
        }
        let parts: Vec<_> = raw.split('.').collect();
        if parts.len() != 3
            || !valid_id(parts[0], 1, 128)
            || !valid_id(parts[1], 1, 128)
            || !valid_id(parts[2], 16, 128)
        {
            return Err(Error::config(
                "ALLOWIT_TOKEN must have the form owner.policy.secret",
            ));
        }
        if policy != parts[1] {
            return Err(Error::config(format!(
                "POLICY {} does not match the policy in ALLOWIT_TOKEN",
                quoted(policy)
            )));
        }
        Ok(Self {
            origin,
            owner: parts[0].into(),
            policy: parts[1].into(),
            token: raw,
            ca_file: env_value("ALLOWIT_CA_FILE"),
        })
    }
    pub fn route(&self, action: &str) -> String {
        format!(
            "{}/api/harness/{}/{}/{}",
            self.origin, self.owner, self.policy, action
        )
    }
    pub fn is_route(&self, action: &str, raw: &str) -> bool {
        if raw.is_empty() || raw.contains(['?', '#', '\\']) {
            return false;
        }
        let Ok(base) = Url::parse(&self.route("skill")) else {
            return false;
        };
        let Ok(u) = base.join(raw) else {
            return false;
        };
        u.username().is_empty()
            && u.password().is_none()
            && u.query().is_none()
            && u.fragment().is_none()
            && u.origin().ascii_serialization() == self.origin
            && u.path() == format!("/api/harness/{}/{}/{action}", self.owner, self.policy)
    }
}
