
## invariants mined from 3 clean runs, checked on buggy runs (3 runs)

| fixture | class | mined | caught | false positive | first violation on bug |
|---|---|---|---|---|---|
| login-session | logic | 3 | missed | no | - |
| counter | logic/state | 7 | ✓ | no | numeric_step POST /api/counter/inc → delta 2 (expected 1) |
| create-500 | network | 8 | ✓ | no | status_always POST /api/todos → status 500 |
| route-mismatch | network/static | 4 | ✓ | no | status_always GET /api/items → status 404 |
| console-crash | console | 6 | ✓ | no | shape_always GET /api/profile → /tags is null (expected array) |
| covered-button | visual/pixel | 0 | n/a (no API traffic) | no | - |
| realtime-sync | multi_actor | 7 | missed | no | - |
| hidden-error | visual | 4 | missed | no | - |

**Valid catches 4/8** from mined invariants alone.
