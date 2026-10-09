//! Versioned source facade. The compiler parses decimal threshold literals exactly.
pub mod prelude {
    pub use crate::prelude::*;
    pub enum Threshold {
        Disabled,
        Score(f64),
        Default(&'static str),
    }
    impl From<f64> for Threshold {
        fn from(value: f64) -> Self {
            Self::Score(value)
        }
    }
    impl From<i32> for Threshold {
        fn from(value: i32) -> Self {
            Self::Score(f64::from(value))
        }
    }
    impl From<Option<f64>> for Threshold {
        fn from(value: Option<f64>) -> Self {
            value.map_or(Self::Disabled, Self::Score)
        }
    }
    pub fn auto(direction: &'static str) -> Threshold {
        Threshold::Default(direction)
    }
    pub(crate) fn threshold(
        value: Threshold,
        direction: &str,
    ) -> Result<(bool, alloc::string::String), PolicyError> {
        let fallback = if direction == "deny" { "40" } else { "85" };
        match value {
            Threshold::Disabled => Ok((false, fallback.into())),
            Threshold::Default(d) if d == direction => Ok((true, fallback.into())),
            Threshold::Score(v) if v.is_finite() && (0.0..=1.0).contains(&v) => {
                let bps = (v * 10_000.0 + 0.00000001) as u64;
                if ((bps as f64 / 10_000.0) - v).abs() > 0.000000001 {
                    return Err(PolicyError {
                        code: "INVALID_POLICY".into(),
                        reason: "Use at most four decimal places.".into(),
                    });
                }
                Ok((true, alloc::format!("{}.{:02}", bps / 100, bps % 100)))
            }
            _ => Err(PolicyError {
                code: "INVALID_POLICY".into(),
                reason: "Use a score in 0..1, None or the matching automatic default.".into(),
            }),
        }
    }
    pub async fn check_preference(
        ctx: &Context,
        question: &str,
        deny: impl Into<Threshold>,
        approve: impl Into<Threshold>,
    ) -> PolicyResult {
        let (deny, below) = threshold(deny.into(), "deny")?;
        let (approve, above) = threshold(approve.into(), "approve")?;
        crate::prelude::check_preference(ctx, question, approve, &above, deny, &below).await
    }
    /// Version 1 Jev checks with decimal preference thresholds.
    pub mod jev {
        pub use crate::prelude::semantic;
        pub use crate::primitives::{check_preference, preference_evidence};
    }
}
