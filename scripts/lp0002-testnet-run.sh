#!/usr/bin/env bash
# LP-0002 msig 2-of-3 FULL flow on the LIVE Logos LEZ v0.2.4 testnet.
# REAL STARK proofs (RISC0_DEV_MODE UNSET). Funder = your pinata-funded public
# account, passed as FUNDER_ID (required; see evidence/README.md).
# Deploy already done manually (program_id reconciled). This runs enroll..assert.
# Gating sized for ~1 block/min. Captures every tx hash to stdout (the run log).
set -uo pipefail

R="${LP0002_ROOT:-$(cd "$(dirname "$0")/.." && pwd)}"
export LEE_WALLET_HOME_DIR="${LEE_WALLET_HOME_DIR:-$R/.testnet-v024/wallet}"
unset RISC0_DEV_MODE   # REAL proofs
cd "$R" || exit 1

BIN=$R/target/release
WALLET=$BIN/wallet
FUNDER_ID="${FUNDER_ID:?set FUNDER_ID to your pinata-funded public account (see evidence/README.md)}"   # the run's funder; OVERRIDE for your own run: `wallet pinata claim --to Public/<id>` then FUNDER_ID=<id>
MEMBER_DUST=5
TREASURY_AMT=100

w()   { RUST_LOG=error "$WALLET" "$@"; }
die() { echo "FATAL: $*" >&2; echo "=== RUN FAILED ==="; exit 1; }

RPC_URL="${RPC_URL:-https://testnet.lez.logos.co}"
# Direct JSON-RPC probe (the wallet CLI's account-get can stall; checks must not depend on it).
rpc_account_initialized() {
  curl -sS -m 15 -X POST "$RPC_URL" -H "Content-Type: application/json" \
    -d "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"getAccount\",\"params\":[\"$1\"]}" 2>/dev/null \
  | python3 -c "import json,sys; r=json.load(sys.stdin)[\"result\"]; sys.exit(0 if r and any(r[\"program_owner\"]) else 1)" 2>/dev/null
}

status_json() { RUST_LOG=error "$BIN/run_read_status" 2>/dev/null | grep -o '{.*}' | tail -1; }
count_now()   { status_json | grep -o '"approval_count":[0-9]*' | grep -o '[0-9]*$'; }
ready_now()   { status_json | grep -o '"ready":[a-z]*' | grep -o 'true\|false'; }

# poll loops sized for ~1 block/min: 40 iters x 15s = 10 min each
wait_ready() {
  for _ in $(seq 1 40); do [ "$(ready_now)" = "true" ] && return 0; sleep 15; done
  die "proposal never became ready (create_proposal not landed)"
}
wait_count() {
  local want=$1 last
  for _ in $(seq 1 40); do last="$(count_now)"; [ "$last" = "$want" ] && return 0; sleep 15; done
  die "approval_count never reached $want (last=${last:-none})"
}
wait_treasury_init() {
  local tid=$1
  for _ in $(seq 1 40); do
    rpc_account_initialized "$tid" && return 0
    sleep 15
  done
  die "treasury PDA never initialized"
}
wait_voting_live() {
  local vid=$1
  for _ in $(seq 1 24); do
    w account get --account-id "Private/$vid" 2>/dev/null | grep -qiv "Uninitialized" \
      && ! w account get --account-id "Private/$vid" 2>/dev/null | grep -qi "Uninitialized" && return 0
    sleep 15
  done
  return 0   # non-fatal; run_approve has its own pre-check
}

echo "=============================================================="
echo "LP-0002 v0.2.4 LIVE TESTNET 2-of-3  (REAL STARK)  start $(date -u +%H:%M:%S)"
echo "funder=$FUNDER_ID  dust=$MEMBER_DUST  treasury=$TREASURY_AMT"
echo "=============================================================="
echo "funder balance:"; w account get --account-id "Public/$FUNDER_ID" 2>&1 | tail -3

# ---- 0. setup voters: import member keychains; capture voting ids (ignore genesis funder) ----
echo "### STEP 0: setup voters (import member 0,1 keychains) $(date -u +%H:%M:%S)"
export VOTERS_DIR="${VOTERS_DIR:-$R/.testnet-demo}"
mkdir -p "$VOTERS_DIR"   # run_setup_voters writes member<i>.keys here and does not create it
SETUP_OUT="$("$BIN/run_setup_voters" 2>&1)" || { echo "$SETUP_OUT"; die "run_setup_voters failed"; }
echo "$SETUP_OUT"
M0_VID="$(printf '%s\n' "$SETUP_OUT" | sed -n 's/^MEMBER0_VOTING_ID=//p' | head -1)"
M1_VID="$(printf '%s\n' "$SETUP_OUT" | sed -n 's/^MEMBER1_VOTING_ID=//p' | head -1)"
[ -n "$M0_VID" ] && [ -n "$M1_VID" ] || die "could not parse member voting ids"
echo "M0_VID=$M0_VID"; echo "M1_VID=$M1_VID"

# ---- 1. enroll x3 (signer-owned registry; no funder needed) ----
echo "### STEP 1: enroll x3 $(date -u +%H:%M:%S)"
"$BIN/run_enroll" 2>&1 || die "run_enroll failed"
echo "waiting ~4 min for 3 enroll txs to land (nonce 0,1,2)..."; sleep 240

# ---- 2. create_proposal (signer-owned proposal acct), wait until ready ----
echo "### STEP 2: create_proposal $(date -u +%H:%M:%S)"
"$BIN/run_create_proposal" 2>&1 || die "run_create_proposal failed"
wait_ready
echo "proposal READY (approval_count=$(count_now)) $(date -u +%H:%M:%S)"

# ---- 3. fund member voting accounts from pinata funder (auth-transfer waits for inclusion) ----
echo "### STEP 3: fund member voting accounts $(date -u +%H:%M:%S)"
echo "-- fund member 0 ($MEMBER_DUST) -> $M0_VID"
w auth-transfer send --from "Public/$FUNDER_ID" --to "Private/$M0_VID" --amount "$MEMBER_DUST" 2>&1 || die "fund member 0 failed"
echo "-- fund member 1 ($MEMBER_DUST) -> $M1_VID"
w auth-transfer send --from "Public/$FUNDER_ID" --to "Private/$M1_VID" --amount "$MEMBER_DUST" 2>&1 || die "fund member 1 failed"
wait_voting_live "$M0_VID"; wait_voting_live "$M1_VID"

# ---- 4. approvals (2-of-3) with REAL STARK proofs ----
echo "### STEP 4a: approve member 0 (REAL STARK ~3min) $(date -u +%H:%M:%S)"
APPROVER_INDEX=0 "$BIN/run_approve" 2>&1 || die "approve(member 0) failed"
wait_count 1
echo "approval_count=1 $(date -u +%H:%M:%S)"
echo "### STEP 4b: approve member 1 (REAL STARK ~3min) $(date -u +%H:%M:%S)"
APPROVER_INDEX=1 "$BIN/run_approve" 2>&1 || die "approve(member 1) failed"
wait_count 2
echo "approval_count=2 THRESHOLD REACHED $(date -u +%H:%M:%S)"

# ---- 5. init treasury + recipient PDAs ----
echo "### STEP 5: init_treasury $(date -u +%H:%M:%S)"
INIT_OUT="$("$BIN/run_init_treasury" 2>&1)" || { echo "$INIT_OUT"; die "init_treasury failed"; }
echo "$INIT_OUT"
TREASURY_ID="$(printf '%s\n' "$INIT_OUT" | sed -n 's/^treasury PDA: *//p' | head -1)"
[ -n "$TREASURY_ID" ] || die "could not parse treasury PDA id"
wait_treasury_init "$TREASURY_ID"
echo "treasury PDA live: $TREASURY_ID $(date -u +%H:%M:%S)"

# ---- 6. fund treasury (plain transfer; PDA now non-default-owned) ----
echo "### STEP 6: fund treasury ($TREASURY_AMT) -> $TREASURY_ID $(date -u +%H:%M:%S)"
w auth-transfer send --from "Public/$FUNDER_ID" --to "Public/$TREASURY_ID" --amount "$TREASURY_AMT" 2>&1 || die "fund treasury failed"
echo "treasury balance after fund:"; w account get --account-id "Public/$TREASURY_ID" 2>&1 | tail -3

# ---- 7. execute (threshold-gated release) ----
echo "### STEP 7: execute $(date -u +%H:%M:%S)"
"$BIN/run_execute" 2>&1 || die "execute failed"

# ---- 8. assert GREEN ----
echo "### STEP 8: assert (count=2, treasury drained, recipient=$TREASURY_AMT) $(date -u +%H:%M:%S)"
ok=0
for _ in $(seq 1 40); do
  if EXPECT_COUNT=2 EXPECT_TREASURY=0 EXPECT_RECIPIENT="$TREASURY_AMT" "$BIN/run_assert_state" 2>&1; then ok=1; break; fi
  sleep 15
done
[ "$ok" = 1 ] || die "run_assert_state did not reach GREEN"
echo "=============================================================="
echo "=== LP-0002 v0.2.4 LIVE TESTNET 2-of-3 GREEN === $(date -u +%H:%M:%S)"
echo "final funder balance:"; w account get --account-id "Public/$FUNDER_ID" 2>&1 | tail -3
echo "final treasury:"; w account get --account-id "Public/$TREASURY_ID" 2>&1 | tail -3
echo "=== RUN COMPLETE ==="
