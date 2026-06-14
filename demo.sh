#!/usr/bin/env bash
# LP-0002 end-to-end demo entrypoint.
#
# Thin wrapper around scripts/lp0002-demo.sh so the demo can be run as
# ./demo.sh from a clean clone (the prize default reproducibility gate).
# Real proofs by default (RISC0_DEV_MODE=0, ~174 s per approve); pass DEV_MODE=1
# for the fast fake-receipt plumbing path. Prerequisites (Rust + the RISC0
# toolchain via rzup install) are documented in README-LP0002.md.
set -euo pipefail
exec "$(dirname "$0")/scripts/lp0002-demo.sh" "$@"
