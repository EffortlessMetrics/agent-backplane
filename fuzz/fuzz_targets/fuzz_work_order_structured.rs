// SPDX-License-Identifier: MIT OR Apache-2.0
//! Fuzz WorkOrder deserialization and builder with structured inputs.
//!
//! Uses the Arbitrary trait to generate diverse WorkOrder field values,
//! exercises the builder, JSON round-trip, and canonical_json.
#![no_main]
use arbitrary::Arbitrary;
use libfuzzer_sys::fuzz_target;

#[derive(Debug, Arbitrary)]
struct WorkOrderInput {
    task: String,
    root: String,
    model: Option<String>,
    max_turns: Option<u32>,
    max_budget: Option<f64>,
    include_globs: Vec<String>,
    exclude_globs: Vec<String>,
    allowed_tools: Vec<String>,
    disallowed_tools: Vec<String>,
    deny_read: Vec<String>,
    deny_write: Vec<String>,
    context_files: Vec<String>,
    snippet_name: String,
    snippet_content: String,
    use_workspace_first: bool,
    use_staged: bool,
    env_keys: Vec<String>,
    env_vals: Vec<String>,
}

fuzz_target!(|input: WorkOrderInput| {
    use abp_core::*;

    // Build via WorkOrderBuilder.
    let mut builder = WorkOrderBuilder::new(&input.task);
    if !input.root.is_empty() {
        builder = builder.root(&input.root);
    }
    if let Some(ref m) = input.model {
        builder = builder.model(m);
    }
    if let Some(t) = input.max_turns {
        builder = builder.max_turns(t);
    }
    if let Some(b) = input.max_budget {
        if b.is_finite() {
            builder = builder.max_budget_usd(b);
        }
    }
    if input.use_workspace_first {
        builder = builder.lane(ExecutionLane::WorkspaceFirst);
    }
    if input.use_staged {
        builder = builder.workspace_mode(WorkspaceMode::Staged);
    } else {
        builder = builder.workspace_mode(WorkspaceMode::PassThrough);
    }
    builder = builder.include(input.include_globs.clone());
    builder = builder.exclude(input.exclude_globs.clone());

    // Set policy.
    builder = builder.policy(PolicyProfile {
        allowed_tools: input.allowed_tools,
        disallowed_tools: input.disallowed_tools,
        deny_read: input.deny_read,
        deny_write: input.deny_write,
        allow_network: vec![],
        deny_network: vec![],
        require_approval_for: vec![],
    });

    // Set context.
    let mut ctx = ContextPacket {
        files: input.context_files,
        snippets: vec![],
    };
    if !input.snippet_name.is_empty() {
        ctx.snippets.push(ContextSnippet {
            name: input.snippet_name.clone(),
            content: input.snippet_content.clone(),
        });
    }
    builder = builder.context(ctx);

    // Set env vars.
    let mut config = RuntimeConfig::default();
    config.model = input.model;
    config.max_turns = input.max_turns;
    config.max_budget_usd = input.max_budget.filter(|b| b.is_finite());
    for (k, v) in input.env_keys.iter().zip(input.env_vals.iter()) {
        config.env.insert(k.clone(), v.clone());
    }
    builder = builder.config(config);

    let wo = builder.build();

    // JSON round-trip must never panic.
    if let Ok(json) = serde_json::to_string(&wo) {
        let _ = serde_json::from_str::<WorkOrder>(&json);
    }

    // canonical_json must never panic.
    let _ = canonical_json(&wo);
});
