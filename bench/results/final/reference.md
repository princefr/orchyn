
## reference (3 runs per variant)

| fixture | class | steps | bug caught | false positive | flaky | network gap | failing step on bug |
|---|---|---|---|---|---|---|---|
| login-session | logic | 8 | yes | no | no | no | #4 expect_text [default]: text "Welcome, alice@test.com" not found in document.body.innerT |
| counter | logic/state | 9 | yes | no | no | no | #6 expect_text [default]: text "Count: 3" not found in document.body.innerText after 3000  |
| create-500 | network | 7 | yes | no | no | yes | #3 expect_text [default]: text "Buy milk" not found in document.body.innerText after 3000  |
| route-mismatch | network/static | 7 | yes | no | no | yes | #1 expect_text [default]: text "Apples" not found in document.body.innerText after 3000 ms |
| console-crash | console | 4 | yes | no | no | no | #2 expect_text [default]: text "admin" not found in document.body.innerText after 3000 ms |
| covered-button | visual/pixel | 4 | yes | no | no | no | #1 click [default]: element #subscribe is covered by DIV.scrim at its centre — a real user |
| realtime-sync | multi_actor | 7 | yes | no | no | no | #5 expect_text [bob]: text "Buy milk" not found in document.body.innerText after 3000 ms |
| hidden-error | visual | 5 | yes | no | no | no | #2 expect_text [default]: text "Email is required" not found in document.body.innerText af |

**Caught 8/8** · false positives 0 · flaky 0 · network-gap signal 2 · mean run 7992 ms
