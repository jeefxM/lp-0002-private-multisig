#!/usr/bin/env bash
# LP-0002 msig 2-of-3 FULL flow on a LOCAL Logos LEZ v0.2.4 sequencer, asserted GREEN.
#
# RISC0_DEV_MODE honoured from env (root demo.sh sets 0 = real STARKs; standalone default 1), LOCAL ONLY — this script NEVER touches testnet.lez.logos.co and performs no
# real on-chain deploy. It boots a throwaway local sequencer, runs the whole multisig lifecycle,
# and exits non-zero unless run_assert_state passes (EXPECT_COUNT=2, treasury drained,
# recipient funded).
#
# Flow:
#   boot sequencer -> setup voters (import funder + members 0,1) -> deploy -> enroll x3
#   -> create_proposal -> shielded-fund member 0 & 1 voting accounts -> approve(0) -> approve(1)
#   -> init_treasury -> fund treasury -> execute -> run_assert_state GREEN.
#
# THE REVIEW-ITEM-#6 FUNDING SOLUTION (the crux):
#   run_approve pre-checks `wallet.check_private_account_initialized(voting_id)` and bails if the
#   member's voting account (`for_regular_private_account(npk(member_secret), VOTE_IDENTIFIER)`) is
#   not LIVE + tracked on the wallet. So before voting, members 0 and 1 each need a live, owned
#   private voting account at identifier 0:
#     * run_setup_voters imports each member's FULL HD-derived KeyChain (msig_demo::member_key_chain)
#       into the wallet key tree, and imports the genesis-funded public funder
#       (testnet_initial_state public account 0, balance 10000, directly spendable).
#     * a shielded transfer `auth-transfer send --from Public/<funder> --to Private/<voting_id>`
#       inits+funds the voting account AND makes the wallet decode/track its EXACT funded state
#       (matching the on-chain commitment) in-tx — no separate sync, no timing gap.
#   run_approve then rides the voting account as `AccountIdentity::PrivateOwned(voting_id)` (the only
#   rider variant whose pre_state the wallet can supply for a demo-keyed account); the circuit still
#   builds the same `PrivateAuthorizedUpdate{nsk, membership_proof}` arm the guest's #6 binds to.
#
# Read-after-mutate ordering is GATED (not slept): create_proposal-ready before approve(0);
# approval_count==1 before approve(1); approval_count==2 before execute; treasury owned-by-
# authenticated_transfer before funding it. Funding/treasury transfers block until landed.
set -uo pipefail

R="${LP0002_ROOT:-$(cd "$(dirname "$0")/.." && pwd)}"
D=$R/.localnet-demo
PORT=3047
MEMBER_DUST=100
TREASURY_AMT=5000
WALLET="$R/target/release/wallet"
BIN="$R/target/release"

export LEE_WALLET_HOME_DIR="$D/wallet"
# Honour an inherited RISC0_DEV_MODE (the root ./demo.sh entrypoint sets 0 = REAL
# STARKs). Standalone default stays 1 (fast fake-receipt logic demo).
export RISC0_DEV_MODE="${RISC0_DEV_MODE:-1}"
cd "$R" || exit 1

SEQ_PID=""
# Stop the throwaway sequencer on ANY exit so it does not keep minting 1s blocks (its RocksDB
# state grows unbounded and will fill the disk if left running between/after demo runs).
cleanup() { [ -n "$SEQ_PID" ] && kill "$SEQ_PID" 2>/dev/null; command -v pkill >/dev/null && pkill -f "sequencer_service.*--port $PORT" 2>/dev/null; true; }
trap cleanup EXIT

die() { echo "FATAL: $*" >&2; [ -f "$D/seq.log" ] && tail -20 "$D/seq.log" >&2; exit 1; }
w()   { RUST_LOG=error "$WALLET" "$@"; }

# Direct JSON-RPC account probe (the wallet CLI's account-get can stall against a
# fresh sequencer; checks must not depend on it).
rpc_account() {
  curl -sS -m 10 -X POST "http://127.0.0.1:$PORT" -H "Content-Type: application/json" \
    -d "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"getAccount\",\"params\":[\"$1\"]}" 2>/dev/null
}
# True when the account exists with a NON-ZERO program_owner (initialized).
rpc_account_initialized() {
  rpc_account "$1" | python3 -c "import json,sys; r=json.load(sys.stdin)[\"result\"]; sys.exit(0 if r and any(r[\"program_owner\"]) else 1)" 2>/dev/null
}
# True when the account has a non-zero balance.
rpc_account_funded() {
  rpc_account "$1" | python3 -c "import json,sys; r=json.load(sys.stdin)[\"result\"]; sys.exit(0 if r and r[\"balance\"]>0 else 1)" 2>/dev/null
}

status_json() { RUST_LOG=error "$BIN/run_read_status" 2>/dev/null | grep -o '{.*}' | tail -1; }
count_now()   { status_json | grep -o '"approval_count":[0-9]*' | grep -o '[0-9]*$'; }
ready_now()   { status_json | grep -o '"ready":[a-z]*' | grep -o 'true\|false'; }

wait_ready() {
  for _ in $(seq 1 40); do [ "$(ready_now)" = "true" ] && return 0; sleep 1; done
  die "proposal never became ready (create_proposal not landed)"
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

# ---- 0. fresh scratch home + sequencer config (testnet_initial_state genesis funds the funder) --
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
# build the standalone local sequencer + the wallet CLI. The `standalone` feature swaps in
# sequencer_core's mock Bedrock/Indexer clients (without it the binary still compiles but dies
# at boot trying to reach those services); neither binary is produced by the runner build above,
# so a clean runner must build them here before the existence checks below.
echo "=== build standalone sequencer + wallet ==="
cargo build --release -p sequencer_service --features standalone 2>&1 | tail -4 || die "sequencer build failed"
cargo build --release -p wallet 2>&1 | tail -4 || die "wallet build failed"
[ -x "$BIN/sequencer_service" ] || die "missing sequencer_service"
[ -x "$WALLET" ] || die "missing wallet binary"

# ---- 1b. build the msig guest ELF from source. The runners embed it via test_methods
# (risc0_build::embed_methods), so the deployed/loaded ELF always matches the committed
# msig.rs source and its MSIG_ID -- no artifact copy, no MSIG_BIN needed.
echo "=== build msig guest ELF (embedded via test_methods) ==="
cargo build --release -p test_methods 2>&1 | tail -2 || die "guest ELF build failed"


# ---- 2. boot local sequencer ----
echo "=== boot local sequencer :$PORT (RISC0_DEV_MODE=$RISC0_DEV_MODE) ==="
command -v pkill >/dev/null && pkill -f "sequencer_service.*--port $PORT" 2>/dev/null; sleep 1
RUST_LOG=info RISC0_DEV_MODE="$RISC0_DEV_MODE" nohup "$BIN/sequencer_service" "$D/sequencer_config.json" --port "$PORT" \
  > "$D/seq.log" 2>&1 &
SEQ_PID=$!
# probe the RPC endpoint directly (works in minimal containers without iproute2)
seq_up() { curl -sS -m 3 -X POST "http://127.0.0.1:$PORT" -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":1,"method":"getChannelId","params":[]}' 2>/dev/null | grep -q result; }
for _ in $(seq 1 40); do seq_up && break; sleep 1; done
seq_up || die "sequencer did not answer RPC on :$PORT"
sleep 2

# ---- 3. initialize wallet storage + import funder + voters; capture ids ----
echo "=== init wallet storage (first-run setup; password is ignored by the wallet storage) ==="
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

# funder must be genesis-funded + directly spendable (testnet_initial_state public acc 0 = 10000)
rpc_account_funded "$FUNDER_ID" \
  || die "funder $FUNDER_ID not funded (sequencer genesis missing testnet_initial_state account 0?)"
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

# ---- 5. fund member voting accounts (shielded; the review-item-#6 live riders) ----
echo "=== fund member 0 voting account ($MEMBER_DUST) -> $M0_VID ==="
w auth-transfer send --from "Public/$FUNDER_ID" --to "Private/$M0_VID" --amount "$MEMBER_DUST" \
  || die "fund member 0 voting account failed"
echo "=== fund member 1 voting account ($MEMBER_DUST) -> $M1_VID ==="
w auth-transfer send --from "Public/$FUNDER_ID" --to "Private/$M1_VID" --amount "$MEMBER_DUST" \
  || die "fund member 1 voting account failed"

# ---- 6. approvals (2-of-3) ----
echo "=== approve as member 0 ==="
APPROVER_INDEX=0 "$BIN/run_approve" || die "approve(member 0) failed"
wait_count 1
echo "approval_count=1"
echo "=== approve as member 1 ==="
APPROVER_INDEX=1 "$BIN/run_approve" || die "approve(member 1) failed"
wait_count 2
echo "approval_count=2 (threshold reached)"

# ---- 7. init treasury + recipient, fund treasury, execute ----
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

# ---- 8. assert GREEN ----
echo "=== assert state (EXPECT_COUNT=2, treasury drained, recipient=$TREASURY_AMT) ==="
ok=0
for _ in $(seq 1 40); do
  if EXPECT_COUNT=2 EXPECT_TREASURY=0 EXPECT_RECIPIENT="$TREASURY_AMT" "$BIN/run_assert_state"; then
    ok=1; break
  fi
  sleep 2
done
[ "$ok" = 1 ] || die "run_assert_state did not reach GREEN"
echo "=== LP-0002 v0.2.4 2-of-3 DEMO GREEN ==="
