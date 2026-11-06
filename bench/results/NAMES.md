# Fixture names

Fixtures were renamed to neutral names so that manifest authors cannot infer
the planted bug from the directory name. Result files written before the
rename (everything in this directory except `invariants/` and later runs) use
the old names:

| old name | new name | bug class |
|---|---|---|
| login-session | login | logic |
| counter | counter | logic/state |
| create-500 | todos | network |
| route-mismatch | shop | network/static |
| console-crash | profile | console |
| covered-button | newsletter | visual/pixel |
| realtime-sync | notes | multi-actor |
| hidden-error | signup | visual |
