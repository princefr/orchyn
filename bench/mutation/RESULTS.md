# Mutation benchmark: Orchyn detectors on realistic bugs

Single-line mutants (`mutate.js`, seed 1) of Web-Bench's reference solutions
for `expressjs`, `survey` and `form`; 8 mutants + the unmutated control per
project (`results/*.jsonl`, mutant details in `results/*-mutants.json`).

- **relevant** — Web-Bench's official tests (tasks 1–15) pass fewer tasks than
  on the control. Mutants whose official run produced no result are excluded.
- **baseline** — `orchyn-check --record` on the control (4 independent-check
  manifests as drivers, actions only, 2 runs), then `--baseline` on the mutant:
  any recorded API or page invariant broken.
- **spec** — the independent checks (written from task text) that pass on the
  control, run on the mutant.
- **universal** — a universal rule that fails on the mutant but not on the
  control.

| | expressjs | survey | form | total |
|---|---|---|---|---|
| relevant mutants | 3 | 4 | 2 | 9 |
| baseline | 3 | 3 | 1 | **7/9** |
| spec checks | (invalid*) | 3 | (none valid) | — |
| universal | 0 | 0 | 0 | 0/9 |
| detector fired on the unmutated control | spec* | — | — | baseline 0/3 |
| baseline fired on non-relevant mutants | 2/5 | 1/3 | 2/4 | 5/12 |

\* The 11 expressjs checks that pass one at a time fail when run together on
the control (state carries over: task 9 registers a user that task 13 then
counts), so the combined spec detector is invalid there.

Reading it:

- A **baseline recorded on a known-good build** caught 7 of the 9 bugs that
  the official tests catch, with no false alarm on the control — no spec and
  no assertion written for the purpose (drivers' assertions are dropped).
- It also flagged 5 mutants the official tests do not catch (e.g. a hidden
  element, an off-by-one number). Those are real behaviour changes the tests
  happen not to cover, not equivalent mutants — but they are unverified as
  user-visible defects.
- Universal rules caught none: these mutants keep pages error-free and
  clickable; universal rules catch crashes, failed requests and covered
  controls (3/8 on the planted-bug suite), not wrong behaviour.
- Small n (9 relevant mutants); treat as a direction, not a rate.
