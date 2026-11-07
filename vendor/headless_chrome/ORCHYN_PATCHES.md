# Orchyn patches to headless_chrome 1.0.21

Vendored from crates.io (`headless_chrome` 1.0.21, MIT, see LICENSE.md) via
`[patch.crates-io]` in the workspace `Cargo.toml`. Only `src/` changes below;
everything else is upstream. Search for `ORCHYN PATCH`.

1. `src/browser/mod.rs` — new-tab handles (`Tab::new`) are built on their own
   thread instead of inside the browser-level event loop, and a failure no
   longer stops that loop.
2. `src/browser/tab/mod.rs` — while `Page.enable` is pending on a new tab, a
   helper thread keeps sending `Page.handleJavaScriptDialog(accept)` to it.

Why: a same-origin popup shares its opener's event loop. If it calls `alert()`
while loading, both pages freeze; `Tab::new` then blocks on `Page.enable`
inside the single browser event thread, and every later call deadlocks
(including the click that opened the popup). Upstream has no way to pause new
targets before their scripts run.
