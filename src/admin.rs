//! Owner operations delegate to the bound vendor implementation.
//!
//! Each capability is independent. A vendor implements only operations for which
//! it has a real owner-authorized preparation path. No default changes rails or
//! substitutes an unsupported operation.

/// The policy configuration version, shared by redeployment and administration.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct PolicyVersion(pub u64);

impl PolicyVersion {
    pub fn next(self) -> Option<Self> {
        self.0.checked_add(1).map(Self)
    }

    /// Reject stale targets, skipped versions and overflow.
    pub fn follows(self, current: Self) -> bool {
        current.next() == Some(self)
    }
}

/// Prepare an owner-reviewed policy redeployment at the next configuration version.
pub trait DeployPolicy {
    type Artifact;
    type Output;
    type Error;
    fn deploy_policy(
        &mut self,
        policy_version: PolicyVersion,
        artifact: Self::Artifact,
    ) -> Result<Self::Output, Self::Error>;
}

/// Prepare the vendor's owner-authorized pause or resume operation.
/// Native replay protection and version changes belong to the vendor.
pub trait SetPause {
    type Output;
    type Error;
    fn set_pause(
        &mut self,
        paused: bool,
        policy_version: PolicyVersion,
    ) -> Result<Self::Output, Self::Error>;
}

/// Recover the selected token balance to the authenticated current owner.
/// The vendor resolves the owner, balance, native call and recovery identity.
pub trait RecoverFunds {
    type Output;
    type Error;
    fn recover_funds(&mut self, token: &str) -> Result<Self::Output, Self::Error>;
}

/// Chain-neutral SDK facade over one explicitly bound vendor.
pub struct Admin<V> {
    vendor: V,
}

impl<V> Admin<V> {
    pub fn new(vendor: V) -> Self {
        Self { vendor }
    }

    pub fn vendor(&self) -> &V {
        &self.vendor
    }

    pub fn into_vendor(self) -> V {
        self.vendor
    }
}

impl<V: DeployPolicy> Admin<V> {
    pub fn deploy_policy(
        &mut self,
        policy_version: PolicyVersion,
        artifact: V::Artifact,
    ) -> Result<V::Output, V::Error> {
        self.vendor.deploy_policy(policy_version, artifact)
    }
}

impl<V: SetPause> Admin<V> {
    pub fn set_pause(
        &mut self,
        paused: bool,
        policy_version: PolicyVersion,
    ) -> Result<V::Output, V::Error> {
        self.vendor.set_pause(paused, policy_version)
    }
}

impl<V: RecoverFunds> Admin<V> {
    pub fn recover_funds(&mut self, token: &str) -> Result<V::Output, V::Error> {
        self.vendor.recover_funds(token)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Vendor {
        seen: alloc::vec::Vec<alloc::string::String>,
    }
    impl DeployPolicy for Vendor {
        type Artifact = &'static str;
        type Output = &'static str;
        type Error = &'static str;
        fn deploy_policy(
            &mut self,
            version: PolicyVersion,
            artifact: Self::Artifact,
        ) -> Result<Self::Output, Self::Error> {
            self.seen
                .push(alloc::format!("deploy:{}:{artifact}", version.0));
            Ok("vendor deployment")
        }
    }
    impl SetPause for Vendor {
        type Output = bool;
        type Error = &'static str;
        fn set_pause(
            &mut self,
            paused: bool,
            policy_version: PolicyVersion,
        ) -> Result<bool, &'static str> {
            self.seen
                .push(alloc::format!("pause:{paused}:{}", policy_version.0));
            Ok(paused)
        }
    }
    impl RecoverFunds for Vendor {
        type Output = ();
        type Error = &'static str;
        fn recover_funds(&mut self, token: &str) -> Result<(), &'static str> {
            self.seen.push(alloc::format!("recover:{token}"));
            Err("owner authorization required")
        }
    }
    #[test]
    fn every_operation_uses_its_vendor_override_and_preserves_errors() {
        let mut admin = Admin::new(Vendor {
            seen: alloc::vec![],
        });
        assert_eq!(
            admin.deploy_policy(PolicyVersion(4), "artifact"),
            Ok("vendor deployment")
        );
        assert_eq!(admin.set_pause(true, PolicyVersion(5)), Ok(true));
        assert_eq!(admin.set_pause(false, PolicyVersion(6)), Ok(false));
        assert_eq!(
            admin.recover_funds("token-address"),
            Err("owner authorization required")
        );
        assert_eq!(
            admin.vendor().seen,
            [
                "deploy:4:artifact",
                "pause:true:5",
                "pause:false:6",
                "recover:token-address"
            ]
        );
    }
    #[test]
    fn versions_reject_replay_skips_and_overflow() {
        assert!(PolicyVersion(8).follows(PolicyVersion(7)));
        assert!(!PolicyVersion(7).follows(PolicyVersion(7)));
        assert!(!PolicyVersion(9).follows(PolicyVersion(7)));
        assert_eq!(PolicyVersion(u64::MAX).next(), None);
        assert!(!PolicyVersion(0).follows(PolicyVersion(u64::MAX)));
    }
}
