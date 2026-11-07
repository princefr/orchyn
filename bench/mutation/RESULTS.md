# Mutation benchmark: Orchyn detectors on realistic bugs

Single-line mutants (`mutate.js`, seed 1) of Web-Bench's reference solutions
for `expressjs`, `survey` and `form`; all 40 mutants + the unmutated control per project (115 of 120 produced an
official-test result) (`results/*.jsonl`, mutant details in `results/*-mutants.json`).

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
| mutants scored (official run produced a result) | 40 | 39 | 36 | 115 of 120 |
| relevant mutants | 15 | 22 | 14 | 51 |
| baseline | 7 | 16 | 10 | **33/51 (65%)** |
| spec checks | (invalid*) | 12 | (none valid) | 12/22 where usable |
| universal | 1 | 0 | 1 | 2/51 |
| baseline alarm on the unmutated control | no | no | no | **0/3** |
| baseline fired on non-relevant mutants | 3/25 | 3/17 | 8/22 | 14/64 |

\* The 11 expressjs checks that pass one at a time fail when run together on
the control (state carries over: task 9 registers a user that task 13 then
counts), so the combined spec detector is invalid there; form's checks pin
the state right after each task and none pass on the final app.

Reading it:

- A **baseline recorded on a known-good build** caught 33 of the 51 bugs that
  the official tests catch (65%; weakest on expressjs, 7/15, where four
  check-driven drivers exercise few server paths), with no false alarm on the control — no spec and
  no assertion written for the purpose (drivers' assertions are dropped).
- It also flagged 14 of 64 mutants the official tests do not catch (e.g. a hidden
  element, an off-by-one number). Those are real behaviour changes the tests
  happen not to cover, not equivalent mutants — but they are unverified as
  user-visible defects.
- Universal rules caught 2/51: these mutants keep pages error-free and
  clickable; universal rules catch crashes, failed requests and covered
  controls (3/8 on the planted-bug suite), not wrong behaviour.
- n = 51 relevant mutants across 3 apps. An earlier partial run (22 relevant)
  suggested 77%; the full set gives 65%.

## More driver flows (expressjs) — earlier result retracted

An earlier version of this section reported that using all 15 checks as
baseline drivers "raised an alarm on the unmutated control". That result is
**invalid**: a server orphaned when a previous run was killed at its time
limit kept the expressjs port, so the later runs' checks hit that stale server
instead of the app under test. The same affected the last 3 expressjs mutants
(m038–m040) of the main table, which were re-scored. `score.sh` now frees the
port before every start. The 15-driver experiment (with a per-flow app reset,
`orchyn-check --reset-cmd`) is being re-run; results below once complete.
