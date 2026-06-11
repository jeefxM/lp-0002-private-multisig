# LP-0002: Anonymous M-of-N Multisig

This is an LP-0002 solution built as a fork of Logos **nssa v0.1.2** (the upstream Logos
Execution Zone). It adds an anonymous M-of-N multisig program to LEZ: a treasury is
controlled by `N` members, and a proposal releases funds once `M` of them approve, with
each individual approval staying **anonymous among the public member set**.

## What it is

The member set is public: anyone can see the `N` enrolled leaves and the frozen
`member_root`. An individual approval, however, reveals nothing about *which* member cast
it. Each `Approve` is a zero-knowledge STARK proving membership in the frozen set, and it
records only a proposal-bound nullifier. The proposal state carries `member_root + proposal_id
+ approval_count` and opaque nullifiers, never any member identity. Two approvals from two
distinct members produce two distinct nullifiers (so the count advances honestly), while a
member who already voted re-derives the same nullifier and is rejected as a double-vote.

## Contribution scope (ours vs upstream)

Everything outside the paths below is upstream Logos nssa v0.1.2, unchanged. See `NOTICE`
for attribution.

Our LP-0002 contribution:

- `programs/msig/core/src/lib.rs` — the `msig_core` shared scheme: depth-5 Merkle member
  set, `MsigInstruction` (`CreateProposal`, `Approve`, `Enroll`, `Execute`, `InitTreasury`),
  domain-separated leaf/nullifier hashing, account layouts.
- `test_program_methods/guest/src/bin/msig.rs` — the on-chain `msig` guest.
- `examples/program_deployment/src/msig_demo.rs` — the shared demo fixture (single source of
  truth for every runner).
- `examples/program_deployment/src/bin/run_{deploy,enroll,init_treasury,create_proposal,approve,execute}.rs`
  — the client runners.
- msig tests in `nssa/src/state.rs` (public-tx + bootstrap + compose) and
  `nssa/src/privacy_preserving_transaction/circuit.rs` (approve tests, including one real
  `RISC0_DEV_MODE=0` STARK plus negatives).
- LP-0002 packaging: this file, `NOTICE`, `scripts/lp0002-demo.sh`,
  `docs/LP-0002-solution.md`, `docs/lp0002-benchmarks.md`, `docs/lp0002-reliability.md`,
  `idl/lp0002-msig.idl.json`, `.github/workflows/lp0002-ci.yml`.

## How to run

```bash
# Self-contained end-to-end demo. The script builds the msig guest ELF, builds and
# boots a local standalone sequencer (genesis-funded payer, rocksdb on a scratch dir),
# then drives the full on-chain flow:
#   deploy -> enroll(x3) -> create_proposal -> approve(member 0) -> approve(member 1)
#   -> init_treasury -> fund -> execute(threshold 2) -> assert (count 2, treasury drained).
# Each approval runs a real ~133s STARK (the default RISC0_DEV_MODE=0 gate).
./scripts/lp0002-demo.sh

# Fast plumbing check with fake receipts (~3 min, no real proofs):
DEV_MODE=1 ./scripts/lp0002-demo.sh
```

The script is self-contained: it boots its own local sequencer and wallet home, so no
external sequencer or testnet access is required. The same flow ran against
`https://testnet.lez.logos.co` to produce the live on-chain evidence below. See the
script header for the per-step `cargo run` invocations if you want to run them manually.

## Deployed program

- Network: `testnet.lez.logos.co`
- Program id (base58): `HjHCub28GrUNgd2QuJ2SPob7YmaUgDRCGXwbt2jt4UWn`
- Program id (8x u32 le): `[1270190072, 1732754497, 1638297940, 148100642, 720938562, 338217588, 1328278959, 865789199]`

## Live on-chain evidence

The full evidence record, with transaction hashes, block numbers, and proving times, is in
[`docs/LP-0002-solution.md`](docs/LP-0002-solution.md). In short:

- **1-of-N full e2e**: deploy, three enrolls (registry root `d0404df3`), treasury bootstrap,
  fund, create_proposal, a real `RISC0_DEV_MODE=0` approve (`13f1f0c2`, ~134s, count 0 -> 1),
  and execute (`2d07a56a`, treasury 100 -> 0, recipient 0 -> 100).
- **2-of-3 threshold** (the M-of-N proof): proposal `BZ182CU`, two anonymous approvals from
  two distinct members (`1bef810a` count 0 -> 1, `05a784ea` count 1 -> 2) with two **distinct**
  vote nullifiers (`cdda374f`, `3979979b`), then execute at threshold=2 (`81c7e42c`, treasury
  20 -> 0, recipient 100 -> 120).

## Further reading

- Instruction layout / IDL: [`idl/lp0002-msig.idl.json`](idl/lp0002-msig.idl.json)
- Benchmarks (proving times): [`docs/lp0002-benchmarks.md`](docs/lp0002-benchmarks.md)
- Reliability / failure modes: [`docs/lp0002-reliability.md`](docs/lp0002-reliability.md)
- CI for the msig paths: [`.github/workflows/lp0002-ci.yml`](.github/workflows/lp0002-ci.yml)

The original upstream nssa README continues below in [`README.md`](README.md).
