# Web-Bench: coding agent with vs. without Orchyn

**Question.** Does giving a coding agent Orchyn's grounded checks make it build
web apps that pass more of a public benchmark's hidden tests?

**Answer from this run: no measurable gain.** With Orchyn the agent passed
71/90 task-tests vs 75/90 shell-only; the difference is within run-to-run noise
(n = 2 per arm), and Orchyn runs took roughly 3–5× longer.

## Setup

- Benchmark: [Web-Bench](https://github.com/bytedance/web-bench) (ByteDance,
  2025, Apache-2.0): 50 projects × 20 sequential tasks, graded by the project's
  own Playwright end-to-end tests. Projects used: `expressjs` (full-stack shop:
  Express 5, Pug, SQLite, JWT), `survey` and `form` (vanilla JS-module apps).
- Agent: Claude Haiku 5.5, one agent per (project, arm, replicate), starting
  from the project's `src-init`, implementing tasks 1–15 in order. Replicate 1
  did tasks 1–5, was scored, then continued with 6–15 in the same workspace;
  replicate 2 got all 15 at once.
- Arms (identical prompts except the verification paragraph):
  - **shell**: verify with node, curl and reading code; no browser automation.
  - **orchyn**: verify with `orchyn-check` (replay manifests the agent writes
    per task, `--universal`, reality gaps); see `ORCHYN.md`.
- Isolation: workspaces contained only the starting code, the task text and
  run instructions — never the tests or the reference solution.
- Scoring: `score.sh` runs the official tests for tasks 1..K against a snapshot
  of the agent's `app/`. Validated first: every reference solution passes 5/5,
  every initial state fails.
- Raw results: `scores.jsonl`.

## Results (tasks 1–15; tasks passed out of 15)

| project | shell rep 1 | shell rep 2 | orchyn rep 1 | orchyn rep 2 |
|---|---|---|---|---|
| expressjs | 13 | 13 | 13 | 13 |
| form | 14 | 14 | 14 | 13 |
| survey | 12 | 9 | 8 | 10 |
| **total** | **39** | **36** | **35** | **36** |

Sequential (Web-Bench's stricter "tasks passed before the first failure"):
shell 5, 5, 11, 11, 7, 5 = 44; orchyn 5, 5, 11, 4, 5, 5 = 35.

Tasks 1–5 alone: every arm and replicate scored 5/5 (ceiling).

Agent wall time per 15 tasks: shell ≈ 4–8 min, orchyn ≈ 14–44 min.

## Why Orchyn did not help here

1. **The remaining failures are mostly spec interpretation, not "the app
   doesn't do what I think".** The agent writes its own checks, so they
   encode its own reading of the task. Survey failures in *both* arms are the
   same misreadings — what "a value from 0 to 1" means for a star rating,
   whether `appendQuestion` goes before or after the submit button, how
   `required` maps to native attributes, design→preview data flow. All of the
   Orchyn agents' self-written checks were green while the hidden tests failed.
2. **Benchmark text vs. tests.** Both expressjs failures, in all four runs,
   are not recoverable from the task text: task 6 asks for `.product_quantity`
   but the test looks for `.product-quantity`; task 13's test expects the
   admin links to be hidden from non-admins, which the task never states. The
   effective ceiling there is 13/15, which every run hit.
3. **Tool gaps found by the agents** (fixed after the run; the agents used
   the pre-fix binary): `alert()` hung `orchyn-check` (now auto-accepted, with
   `expect_dialog`); Express routers mounted with `app.use('/prefix', router)`
   showed up as false reality gaps (now resolved); no drag-and-drop or hover
   op (still missing); generic universal rules fired on legitimate behaviour
   (random titles change on reload; a form posting to an unserved `action`),
   which cost agent time.
4. **Where it did help.** It caught a real bug the agent then fixed: a
   recharge form posting to the wrong URL (404) on expressjs task 12. It did
   not change the final score because the shell agents did not make that
   mistake.

## Caveats

- n = 2 per arm on 3 projects; a 4-point gap out of 90 is noise.
- One Orchyn agent also loaded a self-test page and drove headless Chrome
  directly for drag-and-drop, which blurs the arms.
- Haiku 5.5 is strong enough that tasks 1–5 saturate; a weaker agent or later
  tasks (16–20) may show a different picture.

## What it implies for the idea

Grounded execution feedback is necessary but not sufficient. An agent
checking its own work against its own understanding catches "I wrote it
wrong", not "I understood it wrong". The value has to come from invariants
with an **independent source** — the transcript, contracts in the code,
invariants mined from a known-good version, production traffic — which is
exactly where the internal benchmark shows Orchyn earning its keep
(regression detection), rather than in first-time construction.

## Round 2: independent checks, ambiguity lint, popups

Improvements tested after round 1:

- **Independent checker** — a separate agent writes `checks/task-N.json` from the
  task text *before any code exists*; the coder must make them pass, may not
  edit them, and records conflicts (`indep-checks/`). Coders also got the
  ambiguity linter (`orchyn-check --lint-spec`) and the round-2 tool (dialogs,
  drag, hover, value/count/storage/attribute assertions).
- **v4** (survey only): the checker can follow `window.open` popups
  (`switch_to_popup`) and real `:hover`, so checks follow the real
  design → Preview → new-window flow.

Hidden tests passed (tasks 1–15, two replicates each):

| arm | expressjs | form | survey | total /90 |
|---|---|---|---|---|
| shell only | 13, 13 | 14, 14 | 12, 9 | **75** |
| orchyn, own checks (round 1) | 13, 13 | 14, 13 | 8, 10 | 71 |
| orchyn, independent checks | **14, 14** | 14, 14 | 8, 6 | 70 |
| … survey with v4 popup-capable checks | 14, 14 | 14, 14 | 9, 8 | 73 |

What happened:

- **expressjs: independence beat the "ceiling".** Every other arm hit 13/15
  because task 13's hidden test expects the admin portal links to be hidden
  from non-admins, which the task text never says. The independent checker
  pinned "admin-only" and both coders scored 14/15.
- **survey: independence hurt when the checker guessed wrong.** Checks written
  from text alone encode guesses (e.g. `.q-starCount` as a checkbox; the
  reference uses a number input). On the reference solution only 7/15
  (v1) and 8/15 (v4) of the checker's manifests pass. Coders forced to satisfy
  them drifted from the hidden tests. The remaining survey failures are
  under-specified details (design-mode controls, value encodings) that no
  check written from the text could know.
- **form: no difference** — all arms 13–14/15.
- **Ambiguity lint** flagged all four phrases behind the round-1 failures
  (`lint/`), but in a benchmark there is no human to answer the question, so
  it cannot change the score; it is a product feature, not a benchmark lever.

Tool gaps found and fixed during this round (each found by an agent hitting
it): CSS-only `:hover` menus (now a real pointer move), `window.open` popups
(`switch_to_popup`), clicks racing the navigation they trigger, `alert()`
freezing checks, router mounts reported as false reality gaps, and Chrome
hanging when TMPDIR is long. Known remaining gap, root-caused but not fixed: an `alert()` fired while a
same-origin popup is loading freezes the opener too (they share an event
loop), and headless_chrome's own setup of the new window deadlocks on the
frozen page — so neither the popup listener nor a background watcher can
step in. Fixing it needs Chrome's pause-on-start auto-attach
(`Target.setAutoAttach` with `waitForDebuggerOnStart`), which headless_chrome
does not expose. Two attempts (accept-before-instrument, a watcher thread)
were tried and reverted.

**Bottom line:** checks only help as much as their source is independent
*and* right. An independent author helped where the hidden intent was
guessable (expressjs) and hurt where it was not (survey). Without a human to
resolve ambiguity, the best arm (shell, 75/90) and the best Orchyn arm
(73/90) are within noise.
