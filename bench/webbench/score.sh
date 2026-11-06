#!/usr/bin/env bash
# Usage: wb-score.sh <project> <solution-dir> <K> <port> <label>
# Runs Web-Bench's official Playwright tests for tasks 1..K against solution-dir
# and prints one JSON line: tasks passed, sequential prefix passed, per-file.
set -u
S=/tmp/claude-0/-home-user-orchyn/6c49d970-9dc9-52ac-80e5-e7d17dbfbc63/scratchpad
P=$1; SOL=$(cd "$2" && pwd); K=$3; PORT=$4; LABEL=$5
mkdir -p "$S/wb-score"
W=$(mktemp -d "$S/wb-score/$P-$LABEL-XXXX")
cp -r "$S/web-bench/projects/$P/playwright.config.js" "$S/web-bench/projects/$P/test" "$S/web-bench/projects/$P/package.json" "$W"/
[ -d "$S/web-bench/projects/$P/scripts" ] && cp -r "$S/web-bench/projects/$P/scripts" "$W"/
ln -s "$S/wb-env/node_modules" "$W/node_modules"
cd "$W" || exit 1
files=""; for i in $(seq 1 "$K"); do files="$files test/task-$i.spec.js"; done
# A copy of the solution so the scorer's DB files never touch the agent's tree.
cp -r "$SOL" "$W/solution"
NODE_PATH="$S/wb-env/node_modules" EVAL_PROJECT_ROOT="$W/solution" EVAL_PROJECT_PORT=$PORT IS_EVAL_PRODUCTION=1 \
  timeout 900 npx playwright test $files --workers=1 --reporter=json > result.json 2> stderr.log
node -e '
const fs=require("fs");
let r; try { r=JSON.parse(fs.readFileSync("result.json","utf8")); } catch(e) { console.log(JSON.stringify({project:process.argv[1],label:process.argv[2],error:"no json result"})); process.exit(0); }
const per={};
const walk=s=>{(s.suites||[]).forEach(walk);(s.specs||[]).forEach(sp=>{const f=sp.file.replace(/.*\//,"");const ok=sp.tests.every(t=>t.results.length&&t.results.every(x=>x.status==="passed"));per[f]=(per[f]??true)&&ok;});};
(r.suites||[]).forEach(walk);
const K=+process.argv[3]; let passed=0, prefix=0, stop=false;
for(let i=1;i<=K;i++){const ok=per["task-"+i+".spec.js"]===true; if(ok)passed++; else stop=true; if(!stop)prefix++;}
console.log(JSON.stringify({project:process.argv[1],label:process.argv[2],tasks:K,passed,sequential:prefix,dir:process.cwd()}));
' "$P" "$LABEL" "$K"
