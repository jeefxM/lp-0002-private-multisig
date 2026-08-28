#!/usr/bin/env bash
# LP-0002 msig: PARTIAL-APPROVAL RESUME across a sequencer restart (review item #2),
# on a LOCAL Logos LEZ v0.2.4 standalone sequencer, asserted GREEN.
#
# RISC0_DEV_MODE=1 (fake receipts — this demo shows restart-resume, not proving), LOCAL ONLY — this script NEVER touches testnet.lez.logos.co and performs no
# real on-chain deploy. It is a sibling of scripts/lp0002-demo.sh and reuses the SAME setup,
# funding and runners; the only new thing it proves is durability of approval PROGRESS across a
# crash/restart of the sequencer.
#
# THE LOAD-BEARING CLAIM (review item #2):
#   A 2-of-3 proposal's approval state — member_root + approval_count + the per-member vote
#   nullifiers — lives in the unified ProposalState account on-chain, and the sequencer persists
#   that account state to RocksDB ATOMICALLY on every block (sequencer/core: produce_new_block ->
#   store.update(&block, .., &self.state) -> dbio.atomic_update). On startup, start_from_config does
#   `if rocksdb_path.exists() { open_db; get_lee_state() } else { create_db_with_genesis }`, so a
#   restart against the SAME data dir RESUMES the persisted state (no re-genesis, chain_height
#   continues from latest_block_meta). Therefore a partial approval (count==1) recorded before a
#   kill MUST still read 1 after the sequencer is killed and restarted on the same dir.
#
# WHAT THIS SCRIPT DOES:
#   boot seq (STABLE dir) -> setup voters -> deploy -> enroll x3 -> create_proposal -> fund voters
#   -> approve(member 0) [count 0->1] -> *** kill -9 the sequencer; restart it on the SAME dir ***
#   -> verify run_read_status STILL reports approval_count==1  <-- the load-bearing assertion
#   -> approve(member 1) [count 1->2] -> init_treasury -> fund treasury -> execute
#   -> run_assert_state GREEN.
#
# NOTE on durability timing: store.update() runs INSIDE produce_new_block, under the sequencer lock,
# atomically with the block. run_read_status reads the count over RPC only after that block exists,
# so "count==1 visible" already implies "count==1 persisted to RocksDB" — the kill -9 cannot race
# it. (The zone-sdk/Bedrock checkpoint that IS event-driven only covers bridge deposit/withdraw
# reconciliation, not account/approval state, which is replayed from get_lee_state on reopen.)
#
# What does NOT resume (documented honestly in docs/lp0002-reliability.md, REL-2): an in-flight
# LOCAL approve proof has no client-side checkpoint; if a member's prove is interrupted it re-runs
# from scratch. This script demonstrates the ON-CHAIN half (durable count), which is the property
# that makes re-running a member safe and lets a second member pick up from the persisted count.
set -uo pipefail

R="${LP0002_ROOT:-$(cd "$(dirname "$0")/.." && pwd)}"
D=$R/.localnet-resume          # STABLE data dir: wiped ONCE below, then reused across the restart
PORT=3048                      # distinct from the demo's 3047 so a stray demo seq cannot collide
MEMBER_DUST=100
TREASURY_AMT=5000
WALLET="$R/target/release/wallet"
BIN="$R/target/release"

export LEE_WALLET_HOME_DIR="$D/wallet"
export RISC0_DEV_MODE="${RISC0_DEV_MODE:-1}"
cd "$R" || exit 1

SEQ_PID=""
# Stop the throwaway sequencer on ANY exit so it does not keep minting 1s blocks (its RocksDB
# state grows unbounded and will fill the disk if left running between/after runs).
cleanup() { [ -n "$SEQ_PID" ] && kill "$SEQ_PID" 2>/dev/null; command -v pkill >/dev/null && pkill -f "sequencer_service.*--port $PORT" 2>/dev/null; true; }
trap cleanup EXIT

die() {
  echo "FATAL: $*" >&2
  [ -f "$D/seq.log" ]         && { echo "--- seq.log (boot 1) tail ---" >&2;    tail -20 "$D/seq.log" >&2; }
  [ -f "$D/seq-restart.log" ] && { echo "--- seq-restart.log tail ---" >&2;     tail -20 "$D/seq-restart.log" >&2; }
  exit 1
}
w()   { RUST_LOG=error "$WALLET" "$@"; }

# Direct JSON-RPC account probes (the wallet CLI's account-get can stall against a
# fresh sequencer; checks must not depend on it).
rpc_account() {
  curl -sS -m 10 -X POST "http://127.0.0.1:$PORT" -H "Content-Type: application/json" \
    -d "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"getAccount\",\"params\":[\"$1\"]}" 2>/dev/null
}
rpc_account_initialized() {
  rpc_account "$1" | python3 -c "import json,sys; r=json.load(sys.stdin)[\"result\"]; sys.exit(0 if r and any(r[\"program_owner\"]) else 1)" 2>/dev/null
}
rpc_account_funded() {
  rpc_account "$1" | python3 -c "import json,sys; r=json.load(sys.stdin)[\"result\"]; sys.exit(0 if r and r[\"balance\"]>0 else 1)" 2>/dev/null
}

status_json() { RUST_LOG=error "$BIN/run_read_status" 2>/dev/null | grep -o '{.*}' | tail -1; }
count_now()   { status_json | grep -o '"approval_count":[0-9]*' | grep -o '[0-9]*$'; }
ready_now()   { status_json | grep -o '"ready":[a-z]*' | grep -o 'true\|false'; }

wait_ready() {
  for _ in $(seq 1 40); do [ "$(ready_now)" = "true" ] && return 0; sleep 1; done
  die "proposal never became ready"
}
wait_count() {
  local want=$1 last
  for _ in $(seq 1 60); do last="$(count_now)"; [ "$last" = "$want" ] && return 0; sleep 1; done
  die "approval_count never reached $want (last=${last:-none})"
}
wait_treasury_init() {
  for _ in $(seq 1 40); do
    rpc_account_initialized "$TREASURY_ID" && return 0
    sleep 1
  done
  die "treasury PDA never initialized under authenticated_transfer"
}

# boot_sequencer <logfile>: (re)launch the standalone sequencer against the SAME stable dir $D.
#   * first call: $D/rocksdb does NOT exist  -> start_from_config builds genesis (funder funded).
#   * restart call: $D/rocksdb DOES exist     -> start_from_config OPENS the existing DB and resumes
#     the persisted Lee state (logs "Block cache prepared"), with NO "starting from genesis".
boot_sequencer() {
  local log="$1"
  command -v pkill >/dev/null && pkill -f "sequencer_service.*--port $PORT" 2>/dev/null || true
  for _ in $(seq 1 20); do ss -ltn 2>/dev/null | grep -q ":$PORT" || break; sleep 1; done
  RUST_LOG=info RISC0_DEV_MODE=1 nohup "$BIN/sequencer_service" "$D/sequencer_config.json" --port "$PORT" \
    > "$log" 2>&1 &
  SEQ_PID=$!
  for _ in $(seq 1 40); do ss -ltn 2>/dev/null | grep -q ":$PORT" && break; sleep 1; done
  ss -ltn 2>/dev/null | grep -q ":$PORT" || die "sequencer did not bind :$PORT (see $log)"
  sleep 2
}

# ---- 0. fresh STABLE scratch home (wiped ONCE; the restart REUSES this same dir) ----
rm -rf "$D"; mkdir -p "$D/wallet"
python3 - "$R/lez/sequencer/service/configs/debug/sequencer_config.json" "$D/sequencer_config.json" "$D" <<'PY' || exit 1
import json, sys
src, dst, home = sys.argv[1], sys.argv[2], sys.argv[3]
c = json.load(open(src))
c["home"] = home
c["block_create_timeout"] = "1s"
json.dump(c, open(dst, "w"), indent=2)
PY
# v0.2.4 wallet config schema: multi-sequencer list + client config.
cat > "$D/wallet/wallet_config.json" <<JSON
{
  "sequencers": [ { "sequencer_addr": "http://127.0.0.1:$PORT" } ],
  "seq_poll_timeout": "30s",
  "seq_tx_poll_max_blocks": 25,
  "seq_poll_max_retries": 25,
  "seq_block_poll_max_amount": 300,
  "multi_sequencer_client_config": { "distribution_limit": 1, "calibration_limit": 100 }
}
JSON

# ---- 1. build runners ----
echo "=== build runners ==="
cargo build --release -p program_deployment --bins 2>&1 | tail -4 || die "runner build failed"
for b in run_deploy run_enroll run_create_proposal run_approve run_init_treasury \
         run_execute run_assert_state run_read_status run_setup_voters; do
  [ -x "$BIN/$b" ] || die "missing runner $b"
done
[ -x "$BIN/sequencer_service" ] || die "missing sequencer_service"
[ -x "$WALLET" ] || die "missing wallet binary"

# ---- 1b. refresh the msig guest ELF from source (matches lp0002-demo.sh; the runner build does
# NOT rebuild the guest — runners embed it via test_methods). Fast (cached) on repeat runs. ----
echo "=== refresh msig guest ELF (ABI-fixed chained calls) ==="
cargo build --release -p test_methods 2>&1 | tail -2 || die "guest ELF build failed"

# ---- 2. boot local sequencer (BOOT 1: fresh genesis) ----
echo "=== boot local sequencer :$PORT on STABLE dir $D (RISC0_DEV_MODE=1) ==="
boot_sequencer "$D/seq.log"
grep -q "starting from genesis" "$D/seq.log" || die "boot 1 did not start from genesis (unexpected)"
echo "boot 1: started from genesis (fresh RocksDB)"

# ---- 3. initialize wallet storage + import funder + voters; capture ids ----
echo "=== init wallet storage ==="
printf 'demo\n' | w account list >/dev/null 2>&1 || true
[ -f "$D/wallet/storage.json" ] || die "wallet storage was not initialized"
echo "=== setup voters (import funder + members 0,1) ==="
export VOTERS_DIR="$D"
SETUP_OUT="$("$BIN/run_setup_voters")" || { echo "$SETUP_OUT"; die "run_setup_voters failed"; }
echo "$SETUP_OUT"
FUNDER_ID="$(printf '%s\n' "$SETUP_OUT" | sed -n 's/^FUNDER_ID=//p'         | head -1)"
M0_VID="$(  printf '%s\n' "$SETUP_OUT" | sed -n 's/^MEMBER0_VOTING_ID=//p' | head -1)"
M1_VID="$(  printf '%s\n' "$SETUP_OUT" | sed -n 's/^MEMBER1_VOTING_ID=//p' | head -1)"
[ -n "$FUNDER_ID" ] && [ -n "$M0_VID" ] && [ -n "$M1_VID" ] || die "could not parse setup ids"
rpc_account_funded "$FUNDER_ID" \
  || die "funder $FUNDER_ID not funded/spendable"
echo "funder $FUNDER_ID is funded + spendable"

# ---- 4. deploy -> enroll -> create_proposal ----
echo "=== deploy msig ==="
"$BIN/run_deploy" || die "deploy failed"; sleep 4
echo "=== enroll x3 ==="
"$BIN/run_enroll" || die "enroll failed"; sleep 4
echo "=== create_proposal ==="
"$BIN/run_create_proposal" || die "create_proposal failed"
wait_ready
echo "proposal ready (approval_count=$(count_now))"

# ---- 5. fund member voting accounts (shielded live riders) ----
echo "=== fund member 0 voting account ($MEMBER_DUST) -> $M0_VID ==="
w auth-transfer send --from "Public/$FUNDER_ID" --to "Private/$M0_VID" --amount "$MEMBER_DUST" \
  || die "fund member 0 voting account failed"
echo "=== fund member 1 voting account ($MEMBER_DUST) -> $M1_VID ==="
w auth-transfer send --from "Public/$FUNDER_ID" --to "Private/$M1_VID" --amount "$MEMBER_DUST" \
  || die "fund member 1 voting account failed"

# ---- 6. FIRST approval only (partial: count 0 -> 1) ----
echo "=== approve as member 0 (partial 1-of-2) ==="
APPROVER_INDEX=0 "$BIN/run_approve" || die "approve(member 0) failed"
wait_count 1
PRE_KILL_COUNT="$(count_now)"
echo "approval_count=$PRE_KILL_COUNT (partial; this is the progress that must survive the restart)"

# ---- 7. *** KILL the sequencer, then RESTART it against the SAME data dir *** ----
echo "=== KILL sequencer (pid $SEQ_PID, kill -9 = crash) ==="
kill -9 "$SEQ_PID" 2>/dev/null || true
for _ in $(seq 1 30); do kill -0 "$SEQ_PID" 2>/dev/null || break; sleep 1; done
kill -0 "$SEQ_PID" 2>/dev/null && die "sequencer pid $SEQ_PID did not die"
for _ in $(seq 1 30); do ss -ltn 2>/dev/null | grep -q ":$PORT" || break; sleep 1; done
ss -ltn 2>/dev/null | grep -q ":$PORT" && die "port :$PORT never released after kill"
SEQ_PID=""
echo "sequencer is down; port :$PORT released; RocksDB at $D/rocksdb retained"

echo "=== RESTART sequencer against SAME data dir $D (rocksdb exists -> opens persisted state) ==="
[ -d "$D/rocksdb" ] || die "RocksDB dir vanished — cannot demonstrate restart"
boot_sequencer "$D/seq-restart.log"
# Direct evidence that the restart RESUMED the existing DB rather than re-genesising it:
grep -q "Block cache prepared" "$D/seq-restart.log" \
  || die "restart did not log 'Block cache prepared' (did not reopen existing DB)"
if grep -q "starting from genesis" "$D/seq-restart.log"; then
  die "restart logged 'starting from genesis' — it WIPED state instead of resuming"
fi
echo "restart: reopened existing RocksDB (Block cache prepared; NO re-genesis)"

# ---- 8. *** THE LOAD-BEARING ASSERTION: approval progress survived the restart *** ----
echo "=== verify approval_count STILL == 1 after the restart (RESUME evidence) ==="
wait_ready
RESUMED_COUNT="$(count_now)"
RESUMED_STATUS="$(status_json)"
[ "$RESUMED_COUNT" = "1" ] \
  || die "RESUME FAILED: approval_count=${RESUMED_COUNT:-none} after restart (expected 1)"
echo "RESUME OK: approval_count==1 SURVIVED the sequencer kill+restart"
echo "          (was $PRE_KILL_COUNT before kill; reading $RESUMED_COUNT after restart on the SAME dir)"
echo "          run_read_status now: $RESUMED_STATUS"
echo "          remaining approvals to threshold: $(( $(echo "$RESUMED_STATUS" | grep -o '\"threshold\":[0-9]*' | grep -o '[0-9]*$') - RESUMED_COUNT ))"

# ---- 9. SECOND approval picks up from the resumed count (1 -> 2) ----
echo "=== approve as member 1 (resumes from persisted count -> reaches threshold) ==="
APPROVER_INDEX=1 "$BIN/run_approve" || die "approve(member 1) failed"
wait_count 2
echo "approval_count=2 (threshold reached, built on the count that survived the restart)"

# ---- 10. init treasury + recipient, fund treasury, execute ----
echo "=== init_treasury (+ recipient) ==="
INIT_OUT="$("$BIN/run_init_treasury")" || { echo "$INIT_OUT"; die "init_treasury failed"; }
echo "$INIT_OUT"
TREASURY_ID="$(printf '%s\n' "$INIT_OUT" | sed -n 's/^treasury PDA: *//p' | head -1)"
[ -n "$TREASURY_ID" ] || die "could not parse treasury PDA id"
wait_treasury_init
echo "=== fund treasury ($TREASURY_AMT) -> $TREASURY_ID ==="
w auth-transfer send --from "Public/$FUNDER_ID" --to "Public/$TREASURY_ID" --amount "$TREASURY_AMT" \
  || die "fund treasury failed"
echo "=== execute (threshold-gated treasury release) ==="
"$BIN/run_execute" || die "execute failed"

# ---- 11. assert GREEN ----
echo "=== assert state (EXPECT_COUNT=2, treasury drained, recipient=$TREASURY_AMT) ==="
ok=0
for _ in $(seq 1 40); do
  if EXPECT_COUNT=2 EXPECT_TREASURY=0 EXPECT_RECIPIENT="$TREASURY_AMT" "$BIN/run_assert_state"; then
    ok=1; break
  fi
  sleep 2
done
[ "$ok" = 1 ] || die "run_assert_state did not reach GREEN"
echo "=== LP-0002 v0.2.4 PARTIAL-APPROVAL RESUME GREEN (approval_count==1 survived the restart) ==="
