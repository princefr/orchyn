
## invariants mined from 3 clean runs, checked on buggy runs (3 runs)

| fixture | class | API mined | API | page mined | page | combined | first page violation on bug |
|---|---|---|---|---|---|---|---|
| login | logic | 3 | missed | 25 | ✓ | ✓ | text_always [default@6] Welcome, alice@test.com → text not shown |
| counter | logic/state | 7 | ✓ | 24 | ✓ | ✓ | text_always [bob@3] Count: 2 → text not shown |
| todos | network | 8 | ✓ | 31 | ✓ | ✓ | text_always [bob@10] Buy milk → text not shown |
| shop | network/static | 4 | ✓ | 6 | ✓ | ✓ | text_always [default@0] Apples → text not shown |
| profile | console | 6 | ✓ | 8 | ✓ | ✓ | text_always [default@0] admin → text not shown |
| newsletter | visual/pixel | 0 | n/a | 9 | ✓ | ✓ | text_always [default@1] Subscribed! → step not reached |
| notes | multi_actor | 7 | missed | 27 | ✓ | ✓ | text_always [bob@5] First note → text not shown |
| signup | visual | 4 | missed | 14 | ✓ | ✓ | text_always [default@1] Email is required → text not shown |

**Valid catches** — API 4/8 · pages 8/8 · combined 8/8 (false positives: API 0, pages 0).
