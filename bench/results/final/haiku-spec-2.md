
## haiku-spec-2 (3 runs per variant)

| fixture | class | steps | bug caught | false positive | flaky | network gap | failing step on bug |
|---|---|---|---|---|---|---|---|
| login-session | logic | 19 | yes | no | no | no | #4 expect_text [default]: text "Welcome, alice@test.com" not found in document.body.innerT |
| counter | logic/state | 17 | yes | no | no | no | #3 expect_text [alice]: text "Count: 1" not found in document.body.innerText after 3000 ms |
| create-500 | network | 16 | yes | no | no | yes | #6 expect_text [default]: text "Buy milk" not found in document.body.innerText after 3000  |
| route-mismatch | network/static | 8 | yes | no | no | yes | #3 expect_text [default]: text "Apples" not found in document.body.innerText after 3000 ms |
| console-crash | console | 9 | yes | yes | no | no | #3 expect_text [default]: text "Tags" not found in document.body.innerText after 3000 ms |
| covered-button | visual/pixel | 7 | yes | no | no | no | #1 expect_visible [default]: element #subscribe is covered by DIV.scrim at its centre — a  |
| realtime-sync | multi_actor | 17 | yes | no | no | no | #7 expect_visible [bob]: element #board is not visible (zero size, display:none or visibil |
| hidden-error | visual | 13 | yes | no | no | no | #3 expect_visible [default]: element #email-error is not visible (zero size, display:none  |

**Caught 8/8** · false positives 1 · flaky 0 · network-gap signal 2 · mean run 10310 ms
