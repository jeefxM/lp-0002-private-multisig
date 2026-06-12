#!/usr/bin/env bash
###############################################################################
# VERIFY run_approve_secret: the user's entered secret drives the vote (anti-#87).
# DEV_MODE=1 (fast fake receipts). Boots a fresh standalone sequencer, runs the
# precondition deploy->enroll->create_proposal, then:
#   (a) member 0 secret (a7..a7) -> count 0->1
#   (b) member 1 secret (42..42) -> count 1->2
#   (c) member 2 secret (5c..5c) -> count 2->3   (beyond old 0/1 limit)
#   (d) RANDOM non-member secret -> "not an enrolled member", count UNCHANGED (3)
###############################################################################
set -uo pipefail

DEV_MODE="${DEV_MODE:-1}"
PORT="${PORT:-3055}"
BLOCK_WAIT="${BLOCK_WAIT:-18}"

REPO="/root/lez-v012"
VOL="/mnt/HC_Volume_105854327/lp0002-localnet"
RUN_TS="$(date +%s)"
DATADIR="$VOL/verify-secret-dev${DEV_MODE}-${RUN_TS}"
WALLET_HOME="$VOL/wallet-secret-dev${DEV_MODE}-${RUN_TS}"
SEQ_BIN="$REPO/target/release/sequencer_service"
DEBUG_SEQ_CFG="$REPO/sequencer/service/configs/debug/sequencer_config.json"
DEBUG_WALLET="$REPO/wallet/configs/debug"

M0="a7a7a7a7a7a7a7a7a7a7a7a7a7a7a7a7a7a7a7a7a7a7a7a7a7a7a7a7a7a7a7a7"
M1="4242424242424242424242424242424242424242424242424242424242424242"
M2="5c5c5c5c5c5c5c5c5c5c5c5c5c5c5c5c5c5c5c5c5c5c5c5c5c5c5c5c5c5c5c5c"
NONMEMBER="deadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeef"

cd "$REPO"
SEQ_PID=""
say(){ echo; echo "=== $* ==="; }

cleanup(){
  local rc=$?
  if [ -n "$SEQ_PID" ] && kill -0 "$SEQ_PID" 2>/dev/null; then
    kill "$SEQ_PID" 2>/dev/null || true; wait "$SEQ_PID" 2>/dev/null || true
  fi
  pkill -f "sequencer_service.*--port ${PORT}\b" 2>/dev/null || true
  echo; echo "data dir: $DATADIR"; echo "exit rc=$rc"
}
trap cleanup EXIT

export NSSA_WALLET_HOME_DIR="$WALLET_HOME"
export RISC0_DEV_MODE="$DEV_MODE"

# read proposal approval_count directly (data[64..68] LE u32) via run_assert_state's reader is
# unsafe (asserts PDAs). Use run_approve_secret's own startup banner OR a tiny python over RPC.
# We rely on each run's "live proposal approval_count = N" banner (printed before proving).

say "0. PRE-FLIGHT free port $PORT"
if ss -ltn | grep -q ":${PORT}\b"; then pkill -f "sequencer_service.*--port ${PORT}\b" 2>/dev/null || true; sleep 2; fi

say "3. SEQUENCER CONFIG (home -> $DATADIR)"
mkdir -p "$DATADIR"
python3 - "$DEBUG_SEQ_CFG" "$DATADIR/sequencer_config.json" "$DATADIR" << "PY"
import json,sys
src,dst,home=sys.argv[1],sys.argv[2],sys.argv[3]
d=json.load(open(src)); d["home"]=home
if "initial_accounts" in d:    d["initial_public_accounts"]=d.pop("initial_accounts")
if "initial_commitments" in d: d["initial_private_accounts"]=d.pop("initial_commitments")
json.dump(d,open(dst,"w"),indent=4)
PY

say "4. WALLET HOME ($WALLET_HOME)"
mkdir -p "$WALLET_HOME"
python3 - "$DEBUG_WALLET/storage.json" "$WALLET_HOME/storage.json" << "PY"
import json,sys
src,dst=sys.argv[1],sys.argv[2]
d=json.load(open(src))
if "last_synced_block" in d: d["last_synced_block"]=0
json.dump(d,open(dst,"w"))
PY
python3 - "$DEBUG_WALLET/wallet_config.json" "$WALLET_HOME/wallet_config.json" "$PORT" << "PY"
import json,sys
src,dst,port=sys.argv[1],sys.argv[2],sys.argv[3]
d=json.load(open(src)); d["sequencer_addr"]=f"http://127.0.0.1:{port}"
json.dump(d,open(dst,"w"),indent=2)
PY

say "5. BOOT sequencer (DEV_MODE=$DEV_MODE port $PORT)"
RUST_LOG=info RISC0_DEV_MODE="$DEV_MODE" nohup \
  "$SEQ_BIN" "$DATADIR/sequencer_config.json" --port "$PORT" > "$DATADIR/seq.log" 2>&1 &
SEQ_PID=$!
echo "sequencer pid $SEQ_PID"
for _ in $(seq 1 30); do ss -ltn | grep -q ":${PORT}\b" && break; sleep 1; done
ss -ltn | grep -q ":${PORT}\b" || { echo "seq never bound"; tail -40 "$DATADIR/seq.log"; exit 1; }
sleep 3

run(){ echo "-- $* --"; "$REPO/target/release/$@"; }

say "6a. PRECONDITION deploy -> enroll(x3) -> create_proposal"
run run_deploy;          sleep "$BLOCK_WAIT"
run run_enroll;          sleep "$BLOCK_WAIT"
run run_create_proposal; sleep "$BLOCK_WAIT"

say "(a) APPROVE member 0 secret (a7..a7) -> expect count 0->1"
APPROVER_SECRET_HEX="$M0" run run_approve_secret; sleep "$BLOCK_WAIT"

say "(b) APPROVE member 1 secret (42..42) -> expect count 1->2"
APPROVER_SECRET_HEX="$M1" run run_approve_secret; sleep "$BLOCK_WAIT"

say "(c) APPROVE member 2 secret (5c..5c) -> expect count 2->3  [unlocks >0/1]"
APPROVER_SECRET_HEX="$M2" run run_approve_secret; sleep "$BLOCK_WAIT"

say "(d) APPROVE non-member secret (deadbeef..) -> expect REJECT, count UNCHANGED"
set +e
APPROVER_SECRET_HEX="$NONMEMBER" "$REPO/target/release/run_approve_secret"
NONMEMBER_RC=$?
set -e 2>/dev/null || true
echo "non-member exit code: $NONMEMBER_RC (expect non-zero, no submission)"
sleep "$BLOCK_WAIT"

say "FINAL count read (run_approve_secret member0 banner shows current count, then double-votes)"
set +e
APPROVER_SECRET_HEX="$M0" "$REPO/target/release/run_approve_secret" 2>&1 | grep -E "approval_count|enrolled member index"
set -e 2>/dev/null || true

echo
echo "VERIFY COMPLETE."
