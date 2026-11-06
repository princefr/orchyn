# Orchyn: grounded checks for your running app

`orchyn-check` drives real Chromium browsers against your running app with no AI in
the loop and tells you what a real user would actually see.

    export CHROME=/opt/pw-browsers/chromium-1243/chrome-linux64/chrome ORCHYN_NO_SANDBOX=1
    /tmp/claude-0/-home-user-orchyn/6c49d970-9dc9-52ac-80e5-e7d17dbfbc63/scratchpad/bin/orchyn-check \
        --url http://localhost:<port> --project app \
        --manifest checks/task-1.json [--manifest checks/task-2.json ...] \
        [--universal] [--contracts]

- `--manifest FILE`: a replay manifest = JSON `{"name": "...", "steps": [...]}`. Each step is
  `{"op": ..., ...}`; optional `"actor"` gives a separate browser (own cookies). Ops:
  goto{path}, click{selector} (REAL mouse click at the element centre — fails if the element
  is missing, zero-size or covered by another element), click_text{text}, fill{selector,value},
  wait_ms{ms}, wait_for_text{text,timeout_ms?}, expect_text{text,timeout_ms?} (rendered
  document.body.innerText), expect_no_text{text}, expect_url{contains}, expect_visible{selector},
  expect_all_clickable, expect_console_clean, expect_request{method,path_contains,status?},
  expect_no_failed_requests, reset_telemetry, reload, snapshot_text{key},
  expect_text_unchanged{key}, click_each{selector?}.
  The run halts at the first failing step and prints why.
- `--universal`: checks every page for console errors, failed requests, controls that are
  covered/unclickable, text that changes on reload, buttons whose click causes errors.
- `--contracts`: lists invariants your code declares (required inputs, validation, statuses...).
- Also prints "reality gaps": requests the client code makes that no server route serves,
  or that failed at runtime.

Recommended loop per task: write `checks/task-N.json` encoding every requirement of the task
(exact texts, selectors/classes, URLs, visibility, requests), start the app, run orchyn-check
with ALL checks written so far plus `--universal`, fix the code until everything passes.
