
## haiku-explore-1 (3 runs per variant)

| fixture | class | steps | bug caught | false positive | flaky | network gap | failing step on bug |
|---|---|---|---|---|---|---|---|
| login-session | logic | 19 | yes | no | no | no | #11 expect_text [default]: text "Welcome, alice@test.com" not found in document.body.inner |
| counter | logic/state | 16 | yes | no | no | no | #3 expect_text [default]: text "Count: 1" not found in document.body.innerText after 3000  |
| create-500 | network | 19 | yes | no | no | yes | #7 expect_text [default]: text "Buy milk" not found in document.body.innerText after 3000  |
| route-mismatch | network/static | 7 | yes | no | no | yes | #2 expect_text [default]: text "Apples\nBread" not found in document.body.innerText after  |
| console-crash | console | 8 | yes | no | no | no | #3 expect_text [default]: text "admin\nbeta" not found in document.body.innerText after 30 |
| covered-button | visual/pixel | 8 | yes | no | no | no | #1 expect_visible [default]: element #subscribe is covered by DIV.scrim at its centre — a  |
| realtime-sync | multi_actor | 19 | yes | no | no | no | #11 expect_text [bob]: text "First note" not found in document.body.innerText after 3000 m |
| hidden-error | visual | 14 | yes | no | no | no | #4 expect_text [default]: text "Email is required" not found in document.body.innerText af |

**Caught 8/8** · false positives 0 · flaky 0 · network-gap signal 2 · mean run 10846 ms
