#!/usr/bin/env bash
###############################################################################
# Launch the LP-0002 private-multisig plugin in Logos Basecamp (v0.1.2).
#
# Mirrors LP-0016's run-forum-testnet.sh. Starts the localhost msig sidecar, then
# launches the GUI. The Vote button drives a REAL anonymous on-chain approval:
# MsigBackend -> sidecar /approve -> spawns run_approve_secret (Merkle-membership
# proof + proposal-bound nullifier + submit).
#
# MODE (env, default "app"):
#   app       -> exec the standalone MsigApp (same QML + backend as the plugin;
#                the recordable GUI path LP-0016 uses: it execs ForumApp).
#   basecamp  -> install our module into the EXTRACTED Basecamp tree's
#                usr/plugins/ and launch Basecamp via its AppRun (so the on-disk
#                plugin dir is actually discovered, the raw .AppImage mounts its
#                OWN read-only squashfs and would NOT see an on-disk install).
#
# Prereqs (over VNC, DISPLAY :1):
#   - X display :1 + a VNC server (the user connects via VNC).
#   - A sequencer holding the LP-0002 proposal (deploy -> enroll ->
#     create_proposal landed). Live testnet: committed wallet-home-lp0002. Local
#     demo: boot a standalone sequencer (scripts/lp0002-verify-secret.sh steps
#     3-6a) and point NSSA_WALLET_HOME_DIR + RISC0_DEV_MODE at it.
#   - cargo-built runners at $MSIG_BIN_DIR (run_read_status + run_approve_secret).
#   - node (>=18) for the sidecar.
#
# Usage (over VNC, in a terminal on the box):
#   /root/lez-v012/basecamp/run-basecamp-lp0002.sh             # standalone app
#   MODE=basecamp /root/lez-v012/basecamp/run-basecamp-lp0002.sh   # inside Basecamp
###############################################################################
set -euo pipefail

MODE="${MODE:-app}"
REPO="${MSIG_REPO:-/root/lez-v012}"
PLUGIN_DIR="$REPO/basecamp"
BC_ROOT="${BC_ROOT:-/mnt/HC_Volume_105854327/basecamp-app/squashfs-root}"

# ── chain target ──────────────────────────────────────────────────────────
# Default to the committed LP-0002 wallet home (live testnet). Override both of
# these to point at a local standalone sequencer for a self-contained demo.
export NSSA_WALLET_HOME_DIR="${NSSA_WALLET_HOME_DIR:-$REPO/wallet-home-lp0002}"
export RISC0_DEV_MODE="${RISC0_DEV_MODE:-0}"   # 0 = real STARK gate (~134s/approve)

# ── runners + sidecar ──────────────────────────────────────────────────────
export MSIG_REPO="$REPO"
export MSIG_BIN_DIR="${MSIG_BIN_DIR:-$REPO/target/release}"
export MSIG_SIDECAR_PORT="${MSIG_SIDECAR_PORT:-8799}"
export MSIG_SIDECAR_LOG="${MSIG_SIDECAR_LOG:-$HOME/.vnc/msig-sidecar.log}"

# cargo-installed wallet may be needed on PATH for the runners' deps.
export PATH="$HOME/.cargo/bin:$PATH"

# ── Qt / display ───────────────────────────────────────────────────────────
export DISPLAY="${DISPLAY:-:1}"
export QT_QPA_PLATFORM="${QT_QPA_PLATFORM:-xcb}"
# Tell MsigBackend where to find the sidecar (matches MSIG_SIDECAR_PORT).
export MSIG_SIDECAR_URL="${MSIG_SIDECAR_URL:-http://127.0.0.1:${MSIG_SIDECAR_PORT}}"

for bin in node; do
  command -v "$bin" >/dev/null 2>&1 || { echo "ERROR: '$bin' not on PATH" >&2; exit 1; }
done
[ -x "$MSIG_BIN_DIR/run_approve_secret" ] || { echo "ERROR: run_approve_secret not built at $MSIG_BIN_DIR" >&2; exit 1; }
[ -x "$MSIG_BIN_DIR/run_read_status" ]   || { echo "ERROR: run_read_status not built at $MSIG_BIN_DIR" >&2; exit 1; }

# ── 1. start the sidecar (localhost only) ──────────────────────────────────
echo "Starting msig sidecar on $MSIG_SIDECAR_URL (devMode=$RISC0_DEV_MODE, walletHome=$NSSA_WALLET_HOME_DIR)"
node "$PLUGIN_DIR/msig-sidecar.mjs" &
SIDECAR_PID=$!
trap 'kill "$SIDECAR_PID" 2>/dev/null || true' EXIT
sleep 1
curl -fsS "$MSIG_SIDECAR_URL/health" && echo || { echo "sidecar did not come up"; exit 1; }

# ── 2. launch the GUI ───────────────────────────────────────────────────────
if [ "$MODE" = "basecamp" ]; then
  # Install our packaged module into the EXTRACTED tree's plugin path (the same
  # place the bundled forum/ and counter_qml/ live) and launch via AppRun so the
  # on-disk install is discovered. The .AppImage itself mounts its own read-only
  # squashfs and would NOT see this install, AppRun runs the extracted tree.
  [ -f "$PLUGIN_DIR/build/libmsig_plugin.so" ] || { echo "ERROR: plugin .so not built; run: (cd $PLUGIN_DIR && cmake -B build && cmake --build build)" >&2; exit 1; }
  DEST="$BC_ROOT/usr/plugins/private_multisig_lp0002"
  echo "Installing module -> $DEST"
  rm -rf "$DEST"
  cp -r "$PLUGIN_DIR/dist/private_multisig_lp0002" "$DEST"
  echo "Launching Basecamp (extracted tree) via AppRun"
  exec "$BC_ROOT/AppRun"
else
  # Standalone GUI: same QML + MsigBackend as the Basecamp plugin (LP-0016 uses
  # this exact pattern, its run script execs the standalone ForumApp).
  [ -x "$PLUGIN_DIR/build/MsigApp" ] || { echo "ERROR: MsigApp not built; run: (cd $PLUGIN_DIR && cmake -B build && cmake --build build)" >&2; exit 1; }
  # Load qml/Main.qml from disk so edits are picked up without a rebuild (the
  # Basecamp plugin uses its embedded qrc copy instead).
  export QML_PATH="${QML_PATH:-$PLUGIN_DIR/qml}"
  echo "Launching standalone MsigApp (QML from $QML_PATH)"
  exec "$PLUGIN_DIR/build/MsigApp"
fi
