
## haiku-explore-2 (3 runs per variant)

| fixture | class | steps | bug caught | false positive | flaky | network gap | failing step on bug |
|---|---|---|---|---|---|---|---|
| login-session | logic | 18 | yes | no | no | no | #11 expect_text [default]: text "Welcome, alice@test.com" not found in document.body.inner |
| counter | logic/state | 17 | yes | no | no | no | #4 expect_text [default]: text "Count: 1" not found in document.body.innerText after 3000  |
| create-500 | network | 18 | yes | no | no | yes | #3 expect_request [default]: no POST request to a URL containing "/api/todos" with status  |
| route-mismatch | network/static | 8 | yes | no | no | yes | #2 expect_request [default]: no GET request to a URL containing "/api/items" with status 2 |
| console-crash | console | 8 | yes | no | no | no | #4 expect_text [default]: text "admin\nbeta" not found in document.body.innerText after 30 |
| covered-button | visual/pixel | 9 | yes | no | no | no | #2 expect_visible [default]: element #subscribe is covered by DIV.scrim at its centre — a  |
| realtime-sync | multi_actor | 18 | yes | no | no | no | #6 expect_text [bob]: text "First note" not found in document.body.innerText after 3000 ms |
| hidden-error | visual | 11 | yes | no | no | no | #2 expect_text [default]: text "Email is required" not found in document.body.innerText af |

**Caught 8/8** · false positives 0 · flaky 0 · network-gap signal 2 · mean run 10174 ms
