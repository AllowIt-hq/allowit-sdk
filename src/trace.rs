//! Optional oracle execution evidence. This module is absent from contract builds.
use crate::{
    CompiledPolicy, Decision, SourceSpan, WorkflowStepStatus, WorkflowStepTrace, WorkflowTrace,
};
use alloc::{collections::BTreeMap, vec::Vec};

pub(crate) struct TraceRecorder {
    trace: WorkflowTrace,
    spans: Vec<SourceSpan>,
}

impl TraceRecorder {
    pub(crate) fn new(policy: &CompiledPolicy) -> Self {
        // The freshly compiled workflow uses UTF-16; executable IR uses UTF-8.
        let mut offsets = BTreeMap::new();
        let mut units = 0;
        for (byte, c) in policy.source.char_indices() {
            offsets.insert(units, byte);
            units += c.len_utf16();
        }
        offsets.insert(units, policy.source.len());
        Self {
            trace: WorkflowTrace {
                version: 1,
                source_hash: policy.source_hash.clone(),
                ir_hash: policy.ir_hash.clone(),
                complete: false,
                steps: policy
                    .workflow
                    .iter()
                    .map(|node| WorkflowStepTrace {
                        node_id: node.id.clone(),
                        status: WorkflowStepStatus::Inactive,
                        visited: false,
                    })
                    .collect(),
            },
            spans: policy
                .workflow
                .iter()
                .map(|node| SourceSpan {
                    start: offsets[&node.start],
                    end: offsets[&node.end],
                })
                .collect(),
        }
    }

    pub(crate) fn record(&mut self, span: SourceSpan, decision: Option<&Decision>) {
        let Some(index) = self
            .spans
            .iter()
            .position(|node| span.start >= node.start && span.end <= node.end)
        else {
            return;
        };
        let step = &mut self.trace.steps[index];
        step.visited = true;
        step.status = match decision {
            None => WorkflowStepStatus::Passed,
            Some(d) if d.outcome == "awaiting_input" => WorkflowStepStatus::AwaitingInput,
            Some(d) if d.code == "SEMANTIC_EVIDENCE_REQUIRED" => WorkflowStepStatus::Verifying,
            Some(_) => WorkflowStepStatus::Failed,
        };
    }

    pub(crate) fn finish(mut self, decision: &Decision) -> WorkflowTrace {
        self.trace.complete =
            decision.outcome != "awaiting_input" && decision.code != "SEMANTIC_EVIDENCE_REQUIRED";
        if self.trace.complete {
            for step in &mut self.trace.steps {
                if !step.visited {
                    step.status = WorkflowStepStatus::Skipped;
                }
            }
        }
        self.trace
    }
}
