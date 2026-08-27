# On-chain evidence — how to read and how to regenerate

**The LEZ testnet is periodically wiped and redeployed** (it happened between LEZ
v0.2.0-rc5 and v0.2.4, and it will happen again). A bare transaction hash in a
document therefore has a shelf life: after a wipe it no longer resolves, through
no fault of the submission. This directory exists so the evidence does not rot:

1. **Raw captures** — every `*.json` file here is a raw JSON-RPC request/response
   pair captured from `https://testnet.lez.logos.co` right after the run, with a
   UTC timestamp (`_meta.json` has the capture time). The responses are exactly
   what the chain returned; anyone can replay the `request` field with `curl`
   while the chain still holds the state.
2. **One-command regeneration** — the ledger is reproducible on the CURRENT
   chain at any time:

   ```sh
   # full 2-of-3 flow against the live testnet (real STARKs, ~1h at 1 block/min)
   FUNDER_ID=<your pinata-funded account> ./scripts/lp0002-testnet-run.sh
   # then snapshot the fresh state:
   ./scripts/lp0002-capture-evidence.sh evidence/ledger.txt evidence/
   ```

   Prereqs: `wallet change-network testnet`, `wallet auth-transfer init` +
   `wallet pinata claim --to Public/<funder>`, then `run_deploy` (deploys the
   embedded guest; prints the program id).
3. **`ledger.txt`** — the artifact list (accounts + tx hashes) the capture
   script walks. It doubles as the canonical index of the run.

If the hashes in the main documents do not resolve on the chain you are looking
at, check `_meta.json` for when they were captured, diff the chain tip age, and
re-run the flow — the program deploys and proves identically on the current rev.
