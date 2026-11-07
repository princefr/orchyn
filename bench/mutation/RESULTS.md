# Mutation benchmark: Orchyn detectors on realistic bugs

Single-line mutants (`mutate.js`, seed 1) of Web-Bench's reference solutions
for `expressjs`, `survey` and `form`; up to 20 mutants + the unmutated control per project (the run was
stopped by its 2-hour limit after 54 scorable mutants) (`results/*.jsonl`, mutant details in `results/*-mutants.json`).

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
| mutants scored (official run produced a result) | 17 | 19 | 18 | 54 |
| relevant mutants | 5 | 10 | 7 | 22 |
| baseline | 4 | 8 | 5 | **17/22 (77%)** |
| spec checks | (invalid*) | 8 | (none valid) | 8/10 where usable |
| universal | 0 | 0 | 0 | 0/22 |
| baseline alarm on the unmutated control | no | no | no | **0/3** |
| baseline fired on non-relevant mutants | 2/12 | 1/9 | 4/11 | 7/32 |

\* The 11 expressjs checks that pass one at a time fail when run together on
the control (state carries over: task 9 registers a user that task 13 then
counts), so the combined spec detector is invalid there; form's checks pin
the state right after each task and none pass on the final app.

Reading it:

- A **baseline recorded on a known-good build** caught 17 of the 22 bugs that
  the official tests catch, with no false alarm on the control — no spec and
  no assertion written for the purpose (drivers' assertions are dropped).
- It also flagged 7 of 32 mutants the official tests do not catch (e.g. a hidden
  element, an off-by-one number). Those are real behaviour changes the tests
  happen not to cover, not equivalent mutants — but they are unverified as
  user-visible defects.
- Universal rules caught none: these mutants keep pages error-free and
  clickable; universal rules catch crashes, failed requests and covered
  controls (3/8 on the planted-bug suite), not wrong behaviour.
- n = 22 relevant mutants across 3 apps; a direction more than a precise rate.
