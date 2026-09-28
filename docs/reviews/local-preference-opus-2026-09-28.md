# Local development and preference helper review

Independent review through Claude Code, explicitly selecting and verifying `claude-opus-5-5`. Scope: shared SDK compiler/evaluator, facade, registry and relevant tests only. No app source, credentials or runtime user data was shared.

The first review found no material issue. It identified a misleading question-length diagnostic and the dependence on model evidence when both automatic outcomes were disabled. The implementation now uses the existing 1,024-byte string limit and asks the owner directly in manual-only mode. The second review confirmed these changes and found no material issues. Its recommendation to reject Rust string suffixes is also applied, with regression coverage.

Threshold boundaries, all flag combinations, original function/source spans, nested custom code, caps, missing evidence, reserved variable bindings and Local dev rejection in contract profile are tested. Source helpers lower to existing IR operations, with no new contract opcode. Answers remain a host responsibility scoped to a single immutable request; workflow metadata is a projection from approved source, not authority. Core IR hashing includes source spans.

Review results are static review only. Runtime tests, both contract targets, browser acceptance and native wallet acceptance have separate evidence. The consumer app final source review and agent harness content review are outside this approved source-sharing scope.
