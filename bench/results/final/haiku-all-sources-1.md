
## haiku-all-sources-1 (3 runs per variant)

| fixture | class | steps | bug caught | false positive | flaky | network gap | failing step on bug |
|---|---|---|---|---|---|---|---|
| login-session | logic | 24 | yes | no | no | no | #17 expect_text [default]: text "Welcome, alice@test.com" not found in document.body.inner |
| counter | logic/state | 23 | yes | no | no | no | #10 expect_text [default]: text "Count: 1" not found in document.body.innerText after 3000 |
| create-500 | network | 25 | yes | no | no | yes | #7 expect_text [default]: text "buy milk" not found in document.body.innerText after 3000  |
| route-mismatch | network/static | 15 | yes | no | no | yes | #1 wait_for_text [default]: text "Apples" not found in document.body.innerText after 5000  |
| console-crash | console | 18 | yes | no | no | no | #2 expect_console_clean [default]: actor 'default' has 1 console error(s): Uncaught: TypeE |
| covered-button | visual/pixel | 17 | yes | no | no | no | #3 expect_visible [default]: element #subscribe is covered by DIV.scrim at its centre — a  |
| realtime-sync | multi_actor | 23 | yes | no | no | no | #13 expect_text [bob]: text "hello team" not found in document.body.innerText after 5000 m |
| hidden-error | visual | 21 | yes | no | no | no | #10 expect_visible [default]: element #email-error is not visible (zero size, display:none |

**Caught 8/8** · false positives 0 · flaky 0 · network-gap signal 2 · mean run 11565 ms
