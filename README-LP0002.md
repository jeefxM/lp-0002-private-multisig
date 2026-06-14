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

- `programs/msig/core/src/lib.rs`, the `msig_core` shared scheme: depth-5 Merkle member
  set, `MsigInstruction` (`CreateProposal`, `Approve`, `Enroll`, `Execute`, `InitTreasury`),
  domain-separated leaf/nullifier hashing, account layouts.
- `test_program_methods/guest/src/bin/msig.rs`, the on-chain `msig` guest.
- `examples/program_deployment/src/msig_demo.rs`, the shared demo fixture (single source of
  truth for every runner).
- `examples/program_deployment/src/bin/run_{deploy,enroll,init_treasury,create_proposal,approve,execute}.rs`,
  the client runners.
- msig tests in `nssa/src/state.rs` (public-tx + bootstrap + compose) and
  `nssa/src/privacy_preserving_transaction/circuit.rs` (approve tests, including one real
  `RISC0_DEV_MODE=0` STARK plus negatives).
- LP-0002 packaging: this file, `NOTICE`, `scripts/lp0002-demo.sh`,
  `docs/LP-0002-solution.md`, `docs/lp0002-benchmarks.md`, `docs/lp0002-reliability.md`,
  `idl/lp0002-msig.idl.json`, `.github/workflows/lp0002-ci.yml`.

## Prerequisites

This is a fork of Logos nssa v0.1.2, so it builds like upstream LEZ. You need the Rust
toolchain and the **RISC0 zkVM toolchain**. The RISC0 toolchain provides `r0vm` and the
risc0 guest compiler, which the demo below uses to build the on-chain `msig` guest and to
generate the real STARK at `RISC0_DEV_MODE=0`. Without it the guest build cannot compile.

```sh
# Rust
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
# RISC0 (installs the r0 guest toolchain + r0vm into ~/.risc0)
curl -L https://risczero.com/install | bash
# restart your shell, then:
rzup install
```

The full upstream system dependency list (build-essential, clang, libssl, pkg-config) is in
the main [`README.md`](README.md) under "Install dependencies".

## How to run

```bash
# Self-contained end-to-end demo. The script builds the msig guest ELF, builds and
# boots a local standalone sequencer (genesis-funded payer, rocksdb on a scratch dir),
# then drives the full on-chain flow:
#   deploy -> enroll(x3) -> create_proposal -> approve(member 0) -> approve(member 1)
#   -> init_treasury -> fund -> execute(threshold 2) -> assert (count 2, treasury drained).
# Each approval runs a real ~174s STARK (the default RISC0_DEV_MODE=0 gate).
./scripts/lp0002-demo.sh

# Fast plumbing check with fake receipts (~3 min, no real proofs):
DEV_MODE=1 ./scripts/lp0002-demo.sh
```

The script is self-contained: it boots its own local sequencer and wallet home, so no
external sequencer or testnet access is required. The same flow was exercised against
`https://testnet.lez.logos.co` to produce the live on-chain evidence below. See the
script header for the per-step `cargo run` invocations if you want to run them manually.

## Deployed program

- Network: `testnet.lez.logos.co`
- Program id (base58): `HjHCub28GrUNgd2QuJ2SPob7YmaUgDRCGXwbt2jt4UWn`
- Program id (8x u32 le): `[1270190072, 1732754497, 1638297940, 148100642, 720938562, 338217588, 1328278959, 865789199]`

## Live on-chain evidence

The full evidence record, with transaction hashes and proving times, is in
[`docs/LP-0002-solution.md`](docs/LP-0002-solution.md). In short:

- **2-of-3 threshold** (the M-of-N proof, HD-nsk-derived membership): proposal `Hf84MVjY`
  (member_root `38ea719c`, three HD-derived shielded-account members), two anonymous approvals
  from two distinct members (`09c9cf27` 174.18s count 0 -> 1, `83007dcd` 173.78s count 1 -> 2)
  with two **distinct** vote nullifiers (`748015dc`, `7d37760a`), then InitTreasury
  (`9bfb9fde` / `6696b49d`), fund 20 (`7db0d6c7`), and execute at threshold=2 (`deed4d0c`,
  treasury 20 -> 0, recipient 0 -> 20). Every approve is a real `RISC0_DEV_MODE=0` STARK; any
  hash is verifiable via `wallet chain-info transaction --hash <hash>`.

## Further reading

- Architecture map (component map + flow diagram): [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md)
- Instruction layout / IDL: [`idl/lp0002-msig.idl.json`](idl/lp0002-msig.idl.json)
- Benchmarks (proving times): [`docs/lp0002-benchmarks.md`](docs/lp0002-benchmarks.md)
- Reliability / failure modes: [`docs/lp0002-reliability.md`](docs/lp0002-reliability.md)
- CI for the msig paths: [`.github/workflows/lp0002-ci.yml`](.github/workflows/lp0002-ci.yml)

The original upstream nssa README continues below in [`README.md`](README.md).
