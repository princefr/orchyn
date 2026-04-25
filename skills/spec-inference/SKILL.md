# Spec Inference (Qwen-Local)

## When to use this skill
Triggered when working on `spec-engine`, claim validation, or the prompt for the spec writer agent. Also when tuning confidence calibration for the local Qwen model.

## Core concepts
1. **Spec is a Hypothesis**: The inferred spec is what the code *claims* to do. It is always subordinate to pixel reality.
2. **Disjoint Claim Kinds**: `Feature` (visible), `Constraint` (rule), `Invariant` (always true), `Flow` (sequence). Don't mix them.
3. **Calibrated Confidence**: Qwen must follow the rubric strictly. Overconfidence (0.9 for a weak hint) is a failure.
4. **Evidence Granularity**: Line-level evidence is best. File-level is weak.
5. **Structural Diffing**: Specs are versioned. Compare by stable claim IDs to see what changed between code iterations.

## Conventions
- Declarative SVO sentences: "User can reset password" (Yes) vs "Reset password functionality" (No).
- No implementation details in statements ("Uses bcrypt" is a note, not a claim).
- Flat taxonomy tags: `auth`, `payment`, `email`, `upload`, `realtime`, `a11y`, `i18n`.

## Confidence calibration rubric

| Confidence | Meaning | Example evidence |
|---|---|---|
| 0.95+ | Direct implementation + Test | Route + handler + test file matching |
| 0.80–0.94 | Implementation found | Route + handler logic looks complete |
| 0.60–0.79 | Incomplete evidence | Route exists but handler is stubbed or complex and untested |
| 0.40–0.59 | Indirect evidence | UI string found but no backend logic confirmed |
| 0.20–0.39 | Weak inference | Library import only (e.g. `stripe` in package.json) |
| < 0.20 | Uncertain | Skip/Discard |

## Common pitfalls
- **Feature creep via imports**: Don't assume `stripe` in package.json means payments work. Look for the integration code.
- **Dead Code**: Unreachable handlers are not features.
- **Boilerplate**: Ignore generated template comments and scaffolding.

## Related skills
- `codebase-navigation`
- `gap-detection`
