#!/usr/bin/env bash
# Score Orchyn's detectors on Web-Bench mutants.
#
# Usage: bench/mutation/score.sh <project> <mutants-dir> <checks-dir> <out.jsonl> [max-mutants]
#
# For each mutant (and the unmutated control m000):
#   official   — Web-Bench's official tests for tasks 1..15 (score.sh); a mutant is
#                *relevant* if it passes fewer tasks than the control.
#   spec       — the independent checks that pass on the control, run on the mutant.
#   baseline   — invariants recorded on the control (record) checked on the mutant.
#   universal  — universal rules; detected if a rule fails that passed on the control.
# Each detector is judged per mutant: detected = it reports a failure the control
# did not have.
#
# Env: DRIVERS (task numbers used to drive the baseline, default "5 10 14 15"),
#      MUTANTS (only score these ids, plus the control),
#      ORCHYN_CHECK (binary), WB (web-bench checkout), WBENV (node_modules dir),
#      SCORE (bench/webbench/score.sh), CHROME, ORCHYN_NO_SANDBOX, WORKROOT (scratch dir).
set -u
P=$1; MUT=$(cd "$2" && pwd); CHECKS=$(cd "$3" && pwd); OUT=$4; MAX=${5:-999}
: "${ORCHYN_CHECK:?}" "${WB:?}" "${WBENV:?}" "${SCORE:?}"
PORT=${PORT:-3800}
# Keep our scratch files out of TMPDIR: Chrome puts its profile (and a unix
# socket with a ~108-char path limit) under TMPDIR, and a long TMPDIR makes it
# hang. Use WORKROOT for our own files instead.
WORK=$(mktemp -d "${WORKROOT:-/tmp}/mutscore-$P-XXXX")
log() { echo "[$P] $*" >&2; }

start_app() { # $1 = app dir; starts on $PORT, writes $WORK/pid
  local app=$1
  if [ "$P" = expressjs ]; then
    local w=$WORK/run; rm -rf "$w"; mkdir -p "$w"; cp -r "$app" "$w/app"
    cp -r "$WB/projects/expressjs/scripts" "$w/scripts"; ln -s "$WBENV" "$w/node_modules"
    (cd "$w" && DB_HOST=app/test.sqlite node scripts/init-db.js app >/dev/null 2>&1
     DB_HOST=app/test.sqlite PORT=$PORT node app/index.js >/dev/null 2>&1 & echo $! > "$WORK/pid")
    sleep 2
    [ -x "$CHECKS/seed.sh" ] && bash "$CHECKS/seed.sh" "$PORT" >/dev/null 2>&1
  else
    ("$WBENV/.bin/serve" "$app" -p "$PORT" >/dev/null 2>&1 & echo $! > "$WORK/pid")
    sleep 2
  fi
}
stop_app() { kill "$(cat "$WORK/pid")" 2>/dev/null; sleep 1; }
check() { "$ORCHYN_CHECK" --url "http://localhost:$PORT" --project "$1" "${@:2}"; }

# ── control: which checks are valid, baseline, universal failures ──────────────
CTRL=$MUT/m000
VALID=()
for f in "$CHECKS"/task-*.json; do
  start_app "$CTRL"
  if check "$CTRL" --manifest "$f" >/dev/null 2>&1; then VALID+=("$f"); fi
  stop_app
done
log "valid checks on control: ${#VALID[@]}"
MARGS=(); for f in "${VALID[@]}"; do MARGS+=(--manifest "$f"); done
# The baseline only replays actions (assertions are dropped), so every check can
# drive it, valid or not.
# DRIVERS (task numbers) bounds the cost: each driver is replayed on every
# mutant.
DRIVERS=${DRIVERS:-"5 10 14 15"}
ALL=(); for n in $DRIVERS; do [ -f "$CHECKS/task-$n.json" ] && ALL+=(--manifest "$CHECKS/task-$n.json"); done

start_app "$CTRL"; check "$CTRL" "${ALL[@]}" --record "$WORK/baseline" --runs 2 >/dev/null 2>&1; stop_app
start_app "$CTRL"; check "$CTRL" --universal 2>/dev/null | grep '^FAIL' | sort > "$WORK/u0"; stop_app
CTRL_OFFICIAL=$("$SCORE" "$P" "$CTRL" 15 $((PORT+50)) mut-ctrl | node -e 'let s="";process.stdin.on("data",d=>s+=d).on("end",()=>console.log(JSON.parse(s).passed))')
log "control official tasks passed: $CTRL_OFFICIAL"

n=0
for m in "$MUT"/m[0-9][0-9][0-9]; do
  id=$(basename "$m")
  n=$((n+1)); [ $n -gt "$MAX" ] && break
  # MUTANTS (space-separated ids) restricts scoring to those mutants.
  if [ -n "${MUTANTS:-}" ] && [ "$id" != m000 ] && ! [[ " $MUTANTS " == *" $id "* ]]; then continue; fi
  # Resume: skip mutants already scored in OUT.
  [ -f "$OUT" ] && grep -q "\"mutant\":\"$id\"" "$OUT" && continue
  official=$("$SCORE" "$P" "$m" 15 $((PORT+50)) "mut-$id" | node -e 'let s="";process.stdin.on("data",d=>s+=d).on("end",()=>{try{console.log(JSON.parse(s).passed)}catch(e){console.log(-1)}})')
  if [ ${#VALID[@]} -gt 0 ]; then
    start_app "$m"; check "$m" "${MARGS[@]}" >/dev/null 2>&1; spec=$?; stop_app
  else spec=0; fi
  start_app "$m"; check "$m" "${ALL[@]}" --baseline "$WORK/baseline" >/dev/null 2>&1; base=$?; stop_app
  start_app "$m"; check "$m" --universal 2>/dev/null | grep '^FAIL' | sort > "$WORK/u1"; stop_app
  univ=$(comm -13 "$WORK/u0" "$WORK/u1" | wc -l)
  op=$(node -e 'const m=require(process.argv[1]).find(x=>x.id===process.argv[2]);console.log(m?m.operator:"?")' "$MUT/manifest.json" "$id")
  line="{\"project\":\"$P\",\"mutant\":\"$id\",\"operator\":\"$op\",\"official_passed\":$official,\"control_passed\":$CTRL_OFFICIAL,\"relevant\":$([ "$official" -lt "$CTRL_OFFICIAL" ] && echo true || echo false),\"spec\":$([ $spec -ne 0 ] && echo true || echo false),\"baseline\":$([ $base -ne 0 ] && echo true || echo false),\"universal\":$([ "$univ" -gt 0 ] && echo true || echo false),\"valid_checks\":${#VALID[@]}}"
  echo "$line" | tee -a "$OUT"
done
