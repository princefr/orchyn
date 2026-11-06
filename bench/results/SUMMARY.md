| fixture | class | reference | haiku-spec-1 | haiku-spec-2 | haiku-explore-1 | haiku-explore-2 |
|---|---|---|---|---|---|---|
| login-session | logic | ✓ | ✓ | ✓ | ✓ | ✓ |
| counter | logic/state | ✓ | ✓ | ✓ | ✓ | ✓ |
| create-500 | network | ✓ | ✓ | ✓ | ✓ | ✓ |
| route-mismatch | network/static | ✓ | ✓ | ✓ | ✓ | ✓ |
| console-crash | console | ✓ | ✗ clean fails | ✗ clean fails | ✓ | ✓ |
| covered-button | visual/pixel | ✓ | ✓ | ✓ | ✓ | ✓ |
| realtime-sync | multi_actor | ✓ | ✓ | ✓ | ✓ | ✓ |
| hidden-error | visual | ✓ | ✓ | ✓ | ✓ | ✓ |
| **valid catches** | | **8/8** | **7/8** | **7/8** | **8/8** | **8/8** |
| false positives | | 0 | 1 | 1 | 0 | 0 |
| flaky fixtures | | 0 | 0 | 0 | 0 | 0 |
| runs per variant | | 3 | 3 | 3 | 3 | 3 |
| mean run (ms) | | 7995 | 10633 | 10330 | 10846 | 10174 |
