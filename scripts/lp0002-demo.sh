#!/usr/bin/env bash
###############################################################################
# LP-0002 anonymous M-of-N multisig - local-sequencer end-to-end demo.
#
# From a fresh checkout this script:
#   (a) builds the standalone sequencer + the msig guest ELF + the flow runners,
#   (b) boots a self-contained standalone sequencer on a VOLUME data dir,
#   (c) drives the full flow:
#         deploy -> enroll(x3) -> create_proposal
#         -> approve(member 0) -> approve(member 1)        (the two ZK approvals)
#         -> init_treasury(treasury+recipient) -> fund treasury -> execute(threshold 2),
#   (d) asserts the on-chain outcome (approval_count==2, treasury drains to recipient)
#       and prints the tx / block evidence,
#   (e) cleans up the sequencer.
#
# DEFAULT RISC0_DEV_MODE=0 - the real STARK gate the bounty requires (~174s/approve).
# For fast iteration of the NON-proof logic, run with DEV_MODE=1 (fake receipts):
#         DEV_MODE=1 scripts/lp0002-demo.sh
#
# MUST run under a LOGIN shell so cargo/toolchain is on PATH:
#   ssh hetzner 'bash -lc "cd /root/lez-v012 && scripts/lp0002-demo.sh"'
###############################################################################
set -euo pipefail

# ---- knobs ------------------------------------------------------------------
DEV_MODE="${DEV_MODE:-0}"                 # 0 = real STARK gate (default); 1 = fast/fake receipts
PORT="${PORT:-3040}"
FUND_AMOUNT="${FUND_AMOUNT:-500}"         # non-zero treasury funding -> observable drain
BLOCK_WAIT="${BLOCK_WAIT:-18}"            # seconds to wait per public tx (~15s block cadence)

REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
# Data dir defaults to a tmp path for clean-clone portability; set LP0002_DATA_DIR
# to override (e.g. point at an attached volume when / is tight).
VOL="${LP0002_DATA_DIR:-${TMPDIR:-/tmp}/lp0002-localnet}"
RUN_TS="$(date +%s)"
DATADIR="$VOL/demo-dev${DEV_MODE}-${RUN_TS}"             # FRESH rocksdb dir per run+mode (volume)
WALLET_HOME="$VOL/wallet-home-dev${DEV_MODE}-${RUN_TS}"  # FRESH wallet home per run (volume)
SEQ_BIN="$REPO/target/release/sequencer_service"
DEBUG_SEQ_CFG="$REPO/sequencer/service/configs/debug/sequencer_config.json"
DEBUG_WALLET="$REPO/wallet/configs/debug"               # holds CbgR... genesis key + signing key
CbgR="CbgR6tj5kWx5oziiFptM7jMvrQeYY3Mzaao6ciuhSr2r"     # genesis-funded payer (funds the treasury)

cd "$REPO"
START_EPOCH="$(date +%s)"
SEQ_PID=""

say(){ echo; echo "=== $* ==="; }

cleanup(){
  local rc=$?
  if [ -n "$SEQ_PID" ] && kill -0 "$SEQ_PID" 2>/dev/null; then
    say "CLEANUP: stopping sequencer pid $SEQ_PID"
    kill "$SEQ_PID" 2>/dev/null || true
    wait "$SEQ_PID" 2>/dev/null || true
  fi
  pkill -f "sequencer_service.*--port ${PORT}\b" 2>/dev/null || true
  echo
  if [ "$rc" -eq 0 ]; then echo "RESULT: GREEN (exit 0)"; else echo "RESULT: RED (exit $rc)"; fi
  echo "data dir : $DATADIR  (seq log: $DATADIR/seq.log)"
  echo "wallet   : $WALLET_HOME"
  echo "wall time: $(( $(date +%s) - START_EPOCH ))s"
}
trap cleanup EXIT

# ---- 0. pre-flight: free the port (a stale sequencer serves the wrong chain) ----
say "0. PRE-FLIGHT (port ${PORT}, DEV_MODE=${DEV_MODE})"
if ss -ltn | grep -q ":${PORT}\b"; then
  echo "port ${PORT} busy -> killing stale sequencer"
  pkill -f "sequencer_service.*--port ${PORT}\b" 2>/dev/null || true
  sleep 2
fi

# ---- 0b. ensure logos-blockchain-circuits (the runner build needs them) -----
# `cargo build -p program_deployment` pulls logos-blockchain-pol, whose build
# script requires the circuits release at ~/.logos-blockchain-circuits (or
# $LOGOS_BLOCKCHAIN_CIRCUITS). `rzup` does NOT install these (separate Logos
# release), so fetch the pinned v0.4.2 here if absent -> `git clone && ./demo.sh`
# is turnkey. Pre-installed circuits at the default path are used as-is.
CIRCUITS_DIR="${LOGOS_BLOCKCHAIN_CIRCUITS:-$HOME/.logos-blockchain-circuits}"
if [ ! -d "$CIRCUITS_DIR" ]; then
  case "$(uname -s)-$(uname -m)" in
    Linux-x86_64)   CIRC_ASSET="logos-blockchain-circuits-v0.4.2-linux-x86_64.tar.gz" ;;
    Linux-aarch64)  CIRC_ASSET="logos-blockchain-circuits-v0.4.2-linux-aarch64.tar.gz" ;;
    Darwin-arm64)   CIRC_ASSET="logos-blockchain-circuits-v0.4.2-macos-aarch64.tar.gz" ;;
    *)              CIRC_ASSET="logos-blockchain-circuits-v0.4.2-linux-x86_64.tar.gz" ;;
  esac
  say "0b. INSTALL logos-blockchain-circuits v0.4.2 -> $CIRCUITS_DIR ($CIRC_ASSET)"
  mkdir -p "$CIRCUITS_DIR"
  curl -sSL "https://github.com/logos-blockchain/logos-blockchain-circuits/releases/download/v0.4.2/${CIRC_ASSET}" \
    | tar -xz --strip-components=1 -C "$CIRCUITS_DIR"
fi

# ---- 1. build: msig GUEST ELF (heavy on a clean clone) ----------------------
# cargo test -p nssa --release --no-run compiles the risc0 guest and emits the deployable
# msig.bin at the hardcoded msig_demo::MSIG_BIN path. The committed tree builds program id
# HjHCub28GrUNgd2QuJ2SPob7YmaUgDRCGXwbt2jt4UWn (the deployed id), so this run also revalidates it.
say "1. BUILD guest ELF (cargo test -p nssa --release --no-run)"
cargo test -p nssa --release --no-run

# ---- 2. build: standalone sequencer + flow runners --------------------------
# --features standalone swaps the Bedrock/Indexer clients for no-op mocks (no docker, no
# block-settlement node). The sequencer still produces blocks on its timer + serves full RPC.
say "2. BUILD standalone sequencer + runners"
cargo build --release -p sequencer_service --features standalone
cargo build --release -p program_deployment --bins

# ---- 3. sequencer config: debug config, home -> fresh volume dir, genesis-fund payer ----
# The debug JSON keys initial_accounts/initial_commitments are SILENTLY IGNORED by the typed
# SequencerConfig (fields are initial_public_accounts/initial_private_accounts, no serde alias).
# Rename them so CbgR... is genesis-funded (=10000) and can later fund the treasury.
say "3. SEQUENCER CONFIG (home -> $DATADIR, genesis-fund $CbgR)"
mkdir -p "$DATADIR"
python3 - "$DEBUG_SEQ_CFG" "$DATADIR/sequencer_config.json" "$DATADIR" << "PY"
import json,sys
src,dst,home=sys.argv[1],sys.argv[2],sys.argv[3]
d=json.load(open(src))
d["home"]=home
if "initial_accounts" in d:    d["initial_public_accounts"]=d.pop("initial_accounts")
if "initial_commitments" in d: d["initial_private_accounts"]=d.pop("initial_commitments")
json.dump(d,open(dst,"w"),indent=4)
print("genesis public accounts:",[a["account_id"] for a in d.get("initial_public_accounts",[])])
PY

# ---- 4. wallet home: fresh copy of the debug home (has CbgR signing key), point at local seq ----
# ONE wallet home serves BOTH the self-funded msig runners AND the auth-transfer funding step
# (only the debug home holds CbgR...'s signing key). Copying keeps the original pristine.
# last_synced_block is reset to 0: the wallet CLI (auth-transfer) syncs on startup, and the debug
# storage may carry a height from a prior chain; 0 forces a clean re-sync from this fresh genesis.
say "4. WALLET HOME ($WALLET_HOME -> http://127.0.0.1:${PORT})"
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
d=json.load(open(src))
d["sequencer_addr"]=f"http://127.0.0.1:{port}"
json.dump(d,open(dst,"w"),indent=2)
PY

# ---- 5. boot the standalone sequencer (background) --------------------------
# Sequencer RISC0_DEV_MODE MUST match the clients (it verifies receipts). rocksdb under $DATADIR.
say "5. BOOT sequencer (RISC0_DEV_MODE=$DEV_MODE, port $PORT)"
RUST_LOG=info RISC0_DEV_MODE="$DEV_MODE" nohup \
  "$SEQ_BIN" "$DATADIR/sequencer_config.json" --port "$PORT" \
  > "$DATADIR/seq.log" 2>&1 &
SEQ_PID=$!
echo "sequencer pid $SEQ_PID"
for _ in $(seq 1 30); do ss -ltn | grep -q ":${PORT}\b" && break; sleep 1; done
ss -ltn | grep -q ":${PORT}\b" || { echo "sequencer never bound :$PORT"; tail -40 "$DATADIR/seq.log"; exit 1; }
sleep 3

# ---- 6. drive the msig flow -------------------------------------------------
export NSSA_WALLET_HOME_DIR="$WALLET_HOME"
export RISC0_DEV_MODE="$DEV_MODE"               # client receipts must match the sequencer
run(){ echo "-- $* --"; "$REPO/target/release/$@"; }
step(){ run "$@"; sleep "$BLOCK_WAIT"; }

say "6a. deploy -> enroll(x3) -> create_proposal"
step run_deploy
step run_enroll
step run_create_proposal

say "6b. approve member 0 (ZK)  [RISC0_DEV_MODE=$DEV_MODE]"
APPROVER_INDEX=0 run run_approve; sleep "$BLOCK_WAIT"
say "6c. approve member 1 (ZK)  -> meets THRESHOLD=2"
APPROVER_INDEX=1 run run_approve; sleep "$BLOCK_WAIT"

# init BOTH PDAs (treasury+recipient) under authenticated_transfer; capture treasury id from THIS
# single invocation (do NOT re-run the on-chain init just to read the id).
say "6d. init treasury + recipient PDAs (under authenticated_transfer)"
INIT_OUT="$("$REPO/target/release/run_init_treasury")"
printf '%s\n' "$INIT_OUT"
sleep "$BLOCK_WAIT"
TREASURY_ID="$(printf '%s\n' "$INIT_OUT" | sed -n 's/^treasury PDA:[[:space:]]*//p' | head -1)"
[ -n "$TREASURY_ID" ] || { echo "could not derive treasury id from run_init_treasury output"; exit 1; }

# fund the (now authenticated_transfer-owned) treasury PDA with a non-zero amount so execute is an
# OBSERVABLE drain. The PDA is non-default-owned post-init, so the credit needs no PDA signer.
say "6e. fund treasury $TREASURY_ID with $FUND_AMOUNT (payer $CbgR)"
run wallet auth-transfer send --from "Public/$CbgR" --to "Public/$TREASURY_ID" --amount "$FUND_AMOUNT"
sleep "$BLOCK_WAIT"

say "6f. execute -> drain treasury to recipient at threshold 2"
step run_execute

# ---- 7. assert the on-chain outcome -----------------------------------------
# read-only: approval_count==THRESHOLD(2), treasury==0 (drained), recipient==FUND_AMOUNT.
say "7. ASSERT on-chain outcome"
EXPECT_COUNT=2 EXPECT_TREASURY=0 EXPECT_RECIPIENT="$FUND_AMOUNT" \
  "$REPO/target/release/run_assert_state"

# ---- 8. evidence ------------------------------------------------------------
say "8. EVIDENCE (recent sequencer blocks / tx)"
grep -iE "block|tx|applied|included" "$DATADIR/seq.log" | tail -15 || true

echo
echo "LP-0002 DEMO COMPLETE (DEV_MODE=$DEV_MODE)."
