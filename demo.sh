#!/usr/bin/env bash
# LP-0002 reproducible demo entrypoint — runs the full anonymous 2-of-3 msig
# lifecycle (deploy -> enroll x3 -> create_proposal -> approve x2 -> init_treasury
# -> fund -> execute -> assert) with REAL STARK proofs (RISC0_DEV_MODE=0 by
# default) against a local standalone sequencer it boots itself.
set -euo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
export LP0002_ROOT="$HERE"
exec env RISC0_DEV_MODE="${RISC0_DEV_MODE:-0}" "$HERE/scripts/lp0002-demo.sh"
