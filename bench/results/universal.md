
## universal invariants, no spec (2 runs per variant)

| fixture | class | caught | fired by | invalid on clean app |
|---|---|---|---|---|
| login-session | logic | missed | - | interactions-clean, actors-agree |
| counter | logic/state | missed | - | - |
| create-500 | network | missed | - | - |
| route-mismatch | network/static | ✓ | console-clean, no-failed-requests | - |
| console-crash | console | ✓ | console-clean | - |
| covered-button | visual/pixel | ✓ | all-clickable | - |
| realtime-sync | multi_actor | missed | - | - |
| hidden-error | visual | missed | - | - |

**Caught 3/8** with no spec and no hand-written test.
