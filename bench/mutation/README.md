# Mutation generator for Web-Bench reference projects

`mutate.js` takes a reference solution (`src/` of a Web-Bench project) and writes
small, single-edit mutants of it. The goal is realistic, behaviour-changing bugs
for scoring bug detectors, instead of hand-planted toy faults.

## Usage

```bash
node bench/mutation/mutate.js --src <dir> --out <dir> --project <name> [--max 40] [--seed 1]
```

- Walks `.js .pug .html .css .sql` under `--src` (skips `node_modules`, `*.sqlite`).
- Writes `<out>/<project>/m000/` (unmutated control), `<out>/<project>/m001/` ...
  (one full copy of `--src` per mutant), and `<out>/<project>/manifest.json`.
- Deterministic: the same `--seed` gives the same mutants.
- No npm dependencies. CommonJS, Node 22.

## Operators

| operator | edit |
|---|---|
| `cond_negate` | `if (X)` becomes `if (!(X))` |
| `cmp_flip` | `===`/`!==`, `<`/`>=`, `>`/`<=` swapped (JS only; `=>` and shifts skipped) |
| `num_shift` | integer literal n (not 0) becomes n+1 (JS) |
| `status_code` | `.status(2xx)` becomes `500`; `.status(4xx)` becomes `200` |
| `string_drop` | last word of a prose literal removed (JS strings, pug text, html text) |
| `class_rename` | one class token in `class="..."`, `.className =`, or `classList.add/remove/toggle` gets suffix `-x` |
| `remove_call` | a whole statement line calling `appendChild`, `prepend`, `cookie`, `setItem`, or `addEventListener` is deleted |
| `css_hide` | `display: none;` inserted before the closing brace of a single-class CSS rule whose class is used in markup/JS |
| `route_path` | Express `router.get|post|put|patch|delete('/path')`: last path segment gets suffix `s` |
| `await_drop` | `await` removed (not in `for await`) |
| `return_drop` | `return expr` at statement start becomes `expr` |

Sampling: each operator's candidate sites are shuffled with the seed and interleaved
across files. Mutants are then drawn round-robin over operators until `--max` is
reached. JS mutants are rejected if the mutated file fails to parse (CJS via `vm.Script`,
ESM via a child `node --experimental-vm-modules` probe).

## Output checks

- Each mutant differs from the source in exactly one line (`replace`), one deleted
  line (`remove_call`), or one inserted line (`css_hide`). The generator verifies this
  both in memory and after re-reading the file from disk, and aborts on violation.
- `manifest.json` is `[{id, operator, file, line, before, after}]`. `file` is relative to
  `--src`, `line` is 1-based in the mutant, `before`/`after` are the changed line trimmed
  to 200 characters (empty `before` for `css_hide`, empty `after` for deletions).
  `m000` is not listed in the manifest.
- The script prints candidate counts and selected counts per operator.

## Scoring plan

- A mutant is **relevant** if the project's official Web-Bench tests fail on it, meaning
  the change is behaviour-changing. Not every mutant is relevant. `class_rename` and
  `css_hide` can hit classes nothing depends on, and `string_drop` can hit text no test checks.
- For a detector: **recall** = relevant mutants it flags / relevant mutants.
- **False positive rate** = flagged `m000` (unmutated control) runs / control runs.
  This is measured on `m000` only.
- The relevance labels come from running the official tests against each mutant. This tool
  does not run them and does not read the `test/` directories.
