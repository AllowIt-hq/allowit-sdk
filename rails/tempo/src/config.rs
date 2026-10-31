//! Invocation-scoped request identity without process-wide environment mutation.
pub use allowit_sdk::client::config::parse_origin;
use std::cell::RefCell;
thread_local! {
    static REQUEST_ID: RefCell<Option<String>> = const { RefCell::new(None) };
}
pub fn env_value(name: &str) -> String {
    if name == "ALLOWIT_REQUEST_ID"
        && let Some(value) = REQUEST_ID.with_borrow(Clone::clone)
    {
        return value;
    }
    allowit_sdk::client::config::env_value(name)
}
pub(crate) fn with_request_id<T>(value: Option<&str>, run: impl FnOnce() -> T) -> T {
    struct Restore(Option<String>);
    impl Drop for Restore {
        fn drop(&mut self) {
            REQUEST_ID.with_borrow_mut(|value| *value = self.0.take());
        }
    }
    let _restore = Restore(REQUEST_ID.replace(value.map(str::to_owned)));
    run()
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn scoped_identity_restores_after_nested_call_and_panic() {
        with_request_id(Some("original-request"), || {
            let result = std::panic::catch_unwind(|| {
                with_request_id(Some("nested-request"), || {
                    assert_eq!(env_value("ALLOWIT_REQUEST_ID"), "nested-request");
                    panic!("fixture panic");
                });
            });
            assert!(result.is_err());
            assert_eq!(env_value("ALLOWIT_REQUEST_ID"), "original-request");
        });
    }
    #[test]
    fn concurrent_invocations_keep_distinct_identity() {
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
        let threads: Vec<_> = ["request-one", "request-two"]
            .map(|id| {
                let barrier = barrier.clone();
                std::thread::spawn(move || {
                    with_request_id(Some(id), || {
                        barrier.wait();
                        assert_eq!(env_value("ALLOWIT_REQUEST_ID"), id);
                    })
                })
            })
            .into_iter()
            .collect();
        for thread in threads {
            thread.join().unwrap();
        }
    }
}
