# Gap Detection (Qwen-Local)

## When to use this skill
Triggered when working on `gap-detector`, classification, or severity scoring. This is the final reconciliation of code intent vs pixel reality.

## Core concepts
1. **Reconciled Gaps**: A gap is only a gap if it contradicts a claim or a universal invariant. 
2. **Impact-Based Severity**: Critical (Core flow blocked), High (Major UX break), Medium (Feature degraded), Low (Cosmetic).
3. **Deterministic Reproducers**: Seed + Version + Persona must reproduce the gap.
4. **Fingerprinted Deduplication**: Group gaps by `(claim_id, outcome_hash, page_signature)`.
5. **Trust but Verify**: If claim confidence was < 0.40, don't report `MissingFeature`.

## Gap Taxonomy
- `MissingFeature`: Spec says X exists, pixels show it doesn't.
- `SilentFailure`: Action taken, no visible change.
- `WrongOutcome`: Action produced Y instead of X.
- `UX/Confusion`: Confused actor couldn't find the path.
- `Robustness`: SQLi worked, or 10k chars crashed the app.
- `A11y/I18n`: Contrast fails, or Arabic layout overflows.

## Severity Scoring
- **Critical**: Payment failed, Data lost, App crashed.
- **High**: No error message on failure, Essential field missing.
- **Medium**: Wrong font, Slightly off-center, Slow response.
- **Low**: Typo in non-critical text.

## Conventions
- Gaps must link to `ClaimId`.
- Reproducer includes: `run_seed`, `platform`, `trace_ref`.

## Related skills
- `spec-inference`
- `actor-generation`
- `vlm-prompting`
