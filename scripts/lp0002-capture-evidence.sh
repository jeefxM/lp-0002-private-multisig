#!/usr/bin/env bash
# LP-0002 evidence capture — snapshots the on-chain state backing the submission ledger
# into evidence/*.json as RAW JSON-RPC responses, timestamped. Run right after
# scripts/lp0002-testnet-run.sh (or any time while the testnet still holds the state).
#
# WHY: the LEZ testnet is periodically wiped, which retroactively invalidates bare tx
# hashes in a submission. This folder preserves independently-checkable raw responses
# (and documents the exact requests), and the run script can regenerate a fresh ledger
# on the current chain at any time.
#
# Usage:
#   RPC=https://testnet.lez.logos.co \
#     scripts/lp0002-capture-evidence.sh <ledger-file> <out-dir>
#
# <ledger-file>: text file with lines "account <base58>" / "tx <hash>" / "# comment"
set -euo pipefail
RPC="${RPC:-https://testnet.lez.logos.co}"
LEDGER="${1:?ledger file}"; OUT="${2:?out dir}"
mkdir -p "$OUT"
STAMP="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
echo "{\"captured_at\":\"$STAMP\",\"rpc\":\"$RPC\"}" > "$OUT/_meta.json"
n=0
while read -r kind val _; do
  case "$kind" in
    account) method=getAccount ;;
    tx)      method=getTransaction ;;
    block)   method=getBlock ;;
    ""|"#"*) continue ;;
    *) echo "skip unknown kind: $kind" >&2; continue ;;
  esac
  n=$((n+1))
  req="{\"jsonrpc\":\"2.0\",\"id\":$n,\"method\":\"$method\",\"params\":[\"$val\"]}"
  [ "$kind" = block ] && req="{\"jsonrpc\":\"2.0\",\"id\":$n,\"method\":\"$method\",\"params\":[$val]}"
  resp="$(curl -sS -m 30 -X POST "$RPC" -H "Content-Type: application/json" -d "$req")"
  printf "{\"captured_at\":\"%s\",\"request\":%s,\"response\":%s}\n" "$STAMP" "$req" "$resp" \
    > "$OUT/$(printf %02d $n)-$kind-${val:0:16}.json"
  echo "captured $kind ${val:0:16}… -> $method"
done < "$LEDGER"
echo "=== $n artifacts captured into $OUT (at $STAMP) ==="
