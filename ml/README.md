# Prompt-clarity seed data

`prompt_clarity_seed.jsonl` is synthetic, hand-authored bootstrap data. It is
not user telemetry and must not be presented as evidence of real-user skill.

`prompt_clarity_synthetic_v2.jsonl` is a larger, deterministic synthetic
corpus made locally by `hackai generate-prompt-dataset`. It contains varied
software contexts but is still synthetic; do not call it real-user data.

Labels describe how specific an individual AI-coding request is:

- `new`: little actionable context or success criterion.
- `developing`: a goal plus some context or constraint.
- `proficient`: a bounded task, relevant context, constraints, and a checkable
  outcome.

The classifier measures prompt clarity only. It must not affect PESDO, credit,
eligibility, or moderation. Before any release, add consented, independently
reviewed examples and evaluate false-positive rates across writing styles.

The CLI also emits `slop_likelihood`: a low-confidence local estimate that a
prompt delegates most task framing and decisions to the agent. It is not
AI-authorship detection, and Markdown is not an input. It must not affect
PESDO, credit, eligibility, or moderation.
