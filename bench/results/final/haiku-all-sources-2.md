
## haiku-all-sources-2 (3 runs per variant)

| fixture | class | steps | bug caught | false positive | flaky | network gap | failing step on bug |
|---|---|---|---|---|---|---|---|
| login-session | logic | 25 | yes | no | no | no | #18 expect_text [default]: text "Welcome, alice@test.com" not found in document.body.inner |
| counter | logic/state | 25 | yes | no | no | no | #13 expect_text [idle]: text "Count: 1" not found in document.body.innerText after 3000 ms |
| create-500 | network | 25 | yes | no | no | yes | #15 expect_request [default]: no POST request to a URL containing "/api/todos" with status |
| route-mismatch | network/static | 18 | yes | no | no | yes | #2 wait_for_text [default]: text "Apples" not found in document.body.innerText after 3000  |
| console-crash | console | 13 | yes | no | no | no | #2 expect_text [default]: text "Alice Chen\nTAGS\nadmin\nbeta" not found in document.body. |
| covered-button | visual/pixel | 20 | yes | no | no | no | #2 expect_visible [default]: element #subscribe is covered by DIV.scrim at its centre — a  |
| realtime-sync | multi_actor | 25 | yes | no | no | no | #15 wait_for_text [viewer]: text "Ship the release" not found in document.body.innerText a |
| hidden-error | visual | 20 | yes | no | no | no | #8 expect_text [default]: text "Email is required" not found in document.body.innerText af |

**Caught 8/8** · false positives 0 · flaky 0 · network-gap signal 2 · mean run 13260 ms
