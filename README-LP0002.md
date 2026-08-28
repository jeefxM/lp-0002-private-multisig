# LP-0002: Anonymous M-of-N Multisig

> **v0.2.4 (current testnet rev).** This submission is built on Logos LEZ
> **v0.2.4** and carries the in-circuit live-account binding (review item #6),
> updated for v0.2.1+ viewing-key-bound account ids (the member's ML-KEM vpk is
> part of the private witness). The reproducible demo entrypoint is
> **`./demo.sh`** — it runs the full 2-of-3 lifecycle against a locally-booted
> sequencer with **REAL STARKs (`RISC0_DEV_MODE=0`) by default**; the same inner
> script runs under `RISC0_DEV_MODE=1` in CI for fast logic coverage. The
> live-testnet 2-of-3 evidence was captured on the **v0.2.4 testnet on
> 2026-08-27** (program `4tvD5XPFc4ofgN3YV4ymZ1nWqn3iUwB9tucesYzBKJB9`, deploy
> tx `047668a5…`, two distinct vote nullifiers, treasury drained) — the raw RPC
> snapshots live in **`evidence/`**, and the run is reproducible on the current
> chain at any time (`scripts/lp0002-testnet-run.sh`; see `evidence/README.md`).


This is an LP-0002 solution built as a fork of Logos **LEZ v0.2.4** (the upstream Logos
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

Everything outside the paths below is upstream Logos LEZ v0.2.4, unchanged. See `NOTICE`
for attribution.

Our LP-0002 contribution:

- `programs/msig/core/src/lib.rs`, the `msig_core` shared scheme: depth-5 Merkle member
  set, `MsigInstruction` (`CreateProposal`, `Approve`, `Enroll`, `Execute`, `InitTreasury`),
  domain-separated leaf/nullifier hashing, account layouts.
- `lee/state_machine/test_methods/guest/src/bin/msig.rs`, the on-chain `msig` guest.
- `examples/program_deployment/src/msig_demo.rs`, the shared demo fixture (single source of
  truth for every runner).
- `examples/program_deployment/src/bin/run_{deploy,enroll,init_treasury,create_proposal,approve,execute}.rs`,
  the client runners.
- msig tests in `lee/state_machine/src/state.rs` (public-tx + bootstrap + compose) and
  `lee/state_machine/src/privacy_preserving_transaction/circuit.rs` (approve tests, including one real
  `RISC0_DEV_MODE=0` STARK plus negatives).
- LP-0002 packaging: this file, `NOTICE`, `scripts/lp0002-demo.sh`,
  `docs/LP-0002-solution.md`, `docs/lp0002-benchmarks.md`, `docs/lp0002-reliability.md`,
  `idl/lp0002-msig.idl.json`, `.github/workflows/lp0002-ci.yml`.

## Prerequisites

This is a fork of Logos LEZ v0.2.4, so it builds like upstream LEZ. You need the Rust
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

No separate circuits download is needed on v0.2.4: the privacy circuit and all
guest ELFs are compiled/embedded by the cargo build itself (`risc0_build`), and
the live-testnet run in `evidence/` was produced with exactly this build path.

## How to run

```bash
# Self-contained end-to-end demo. The script builds the msig guest ELF, builds and
# boots a local standalone sequencer (genesis-funded payer, rocksdb on a scratch dir),
# then drives the full on-chain flow:
#   deploy -> enroll(x3) -> create_proposal -> approve(member 0) -> approve(member 1)
#   -> init_treasury -> fund -> execute(threshold 2) -> assert (count 2, treasury drained).
# Each approval runs a REAL STARK (RISC0_DEV_MODE=0 — ~30 min per approve on an
# 8-vCPU host; the v0.2.4 privacy circuit is ~4x the rc5 one, and the outer
# prover needs >6 GB RAM).
./demo.sh

# Fast plumbing check with fake receipts (minutes, no real proofs — this is what
# CI runs; the inner script alone defaults to DEV_MODE=1 for iteration):
RISC0_DEV_MODE=1 ./scripts/lp0002-demo.sh
```

The script is self-contained: it boots its own local sequencer and wallet home, so no
external sequencer or testnet access is required. The same flow was exercised against
`https://testnet.lez.logos.co` to produce the live on-chain evidence below. To run the flow by hand (one runner per step) instead of via the script, see the **Manual CLI walkthrough** below.

## Manual CLI walkthrough

`scripts/lp0002-demo.sh` wraps the per-step runners below. To drive the flow by hand against a
sequencer (local or testnet), point a wallet home at the target and run the runners in order. Every
runner reads the sequencer address from the wallet config at `$LEE_WALLET_HOME_DIR`
(`WalletCore::from_env`); `run_approve` additionally honours `RISC0_DEV_MODE` and `APPROVER_INDEX`,
and `run_assert_state` honours `EXPECT_*`.

| Step | Runner | Tx kind | Key env | What it does |
|------|--------|---------|---------|--------------|
| 1 | `run_deploy` | deploy | -- | Deploys `msig.bin`; prints the program id (RISC0 image id). |
| 2 | `run_enroll` | public | -- | One `Enroll` tx per demo member; builds `member_root`. |
| 3 | `run_create_proposal` | public | -- | Claims + freezes the `ProposalState` at `member_root` (count 0). |
| 4 | `run_approve` | privacy (ZK) | `APPROVER_INDEX`, `RISC0_DEV_MODE` | Anonymous approval: in-guest membership proof + proposal-bound nullifier; `approval_count++`. Run once per approving member. |
| 5 | `run_init_treasury` | public | -- | Bootstraps the treasury + recipient PDAs (prints `treasury PDA: <id>`). |
| 6 | `run_execute` | public | -- | At `approval_count >= threshold`, drains the treasury PDA to the recipient. |
| -- | `run_assert_state` | read-only | `EXPECT_COUNT`, `EXPECT_TREASURY`, `EXPECT_RECIPIENT` | Asserts the on-chain outcome; exits non-zero on mismatch. |

Each runner is invoked as `cargo run --release -p program_deployment --bin <runner>`. For example, a
real (`RISC0_DEV_MODE=0`) approval by member 0:

```sh
LEE_WALLET_HOME_DIR=/path/to/wallet-home \
  RISC0_DEV_MODE=0 APPROVER_INDEX=0 \
  cargo run --release -p program_deployment --bin run_approve
```

Between steps 5 and 6, fund the treasury with the wallet CLI (the payer holds the signing key; the
treasury PDA is non-default-owned after `run_init_treasury`, so the credit needs no PDA signer):

```sh
wallet auth-transfer send --from Public/<payer> --to Public/<treasury> --amount 500
```

`run_assert_state` then prints the load-bearing green/red gate:

```
ASSERT proposal <id>: approval_count=2 (expect 2)
ASSERT treasury <id>: balance=0 (expect 0)
ASSERT recipient <id>: balance=500 (expect 500)
ALL ASSERTIONS PASSED
```

> The runners perform ON-CHAIN actions when run; a plain `cargo build` is always safe. Against the
> local standalone sequencer, prefer `./demo.sh` (real STARKs) or
> `RISC0_DEV_MODE=1 ./scripts/lp0002-demo.sh` (fast no-proof plumbing check),
> which wire all of the above together and assert the result.

## Basecamp module

LP-0002 also ships a Basecamp UI module (`private_multisig_lp0002`, `type: ui_qml`): a
Qt6/QML front-end over the same flow that talks to a localhost sidecar. See the module’s
**[`README.md`](https://github.com/jeefxM/logos-lp0002-msig-module#readme)** for the install,
build-from-source, and run-the-demo instructions plus the localhost sidecar contract. A prebuilt, installable **multi-variant** package (`darwin-arm64` + `linux-amd64` + `linux-arm64`,
**Ed25519-signed**; portable — Qt resolved from the host Basecamp, no Nix/store paths) is **hosted as
a downloadable `.lgx`** at
**https://github.com/jeefxM/logos-lp0002-msig-module/releases/latest** — install via Basecamp ->
Package Manager -> *Install from file*. The module source is the public repo
[`jeefxM/logos-lp0002-msig-module`](https://github.com/jeefxM/logos-lp0002-msig-module).

> Provenance: the hosted `.lgx` was built, signed, and verified in the
> first-round rework (review item #5). It has not been re-verified against the
> v0.2.4 runners or the current Basecamp release; the v0.2.4 protocol evidence
> is the CLI + live-testnet ledger in this repo.

## Deployed program

- Network: `testnet.lez.logos.co`
- Program id (base58): `4tvD5XPFc4ofgN3YV4ymZ1nWqn3iUwB9tucesYzBKJB9`. **Live
  deploy/evidence: ✅ captured 2026-08-27 on the v0.2.4 testnet** — deploy tx
  `047668a5ba871873645c2ff412414dfbe79526e1c205ccf5dd0baa474e865df1`; the live
  proposal account is owned by this program id (raw RPC snapshots in `evidence/`)
- Program id (8x u32 le): `[1155063609, 1918607948, 1043343914, 2266441241, 1831314946, 53341822, 1565811176, 2148869898]`

## Live on-chain evidence

The full evidence record, with transaction hashes and proving times, is in
[`docs/LP-0002-solution.md`](docs/LP-0002-solution.md). In short:

- **2-of-3 threshold** (the M-of-N proof, HD-nsk-derived membership): proposal
  `Hf84MVjYamaaCxmBpziYEow6JNuLH7SBNdzLwArf23vu` (member_root `fe674331`, three
  HD-derived shielded-account members with vpk-bound voting ids), two anonymous
  approvals from two distinct members (`f86ffd5d` count 0 -> 1,
  `2ae72df7` count 1 -> 2) with two **distinct** proposal-bound vote
  nullifiers (`a139609a`, `0e491ba7`), then InitTreasury (`41a0ba11`/`fd37efb3`), fund 100
  (`67c39319`), and execute at threshold=2 (`ded1cec1`, treasury 100 -> 0,
  recipient 0 -> 100). Deploy tx `047668a5`. Every approve is a real
  `RISC0_DEV_MODE=0` STARK (inner msig guest 1,048,576 cycles ≈4.6 min + outer
  privacy circuit ≈24 min on an 8-vCPU host, succinct proof ≈261 KB); any hash is
  verifiable via `wallet chain-info transaction --hash <hash>`, and the raw RPC
  responses are committed under `evidence/`.

## Further reading

- Instruction layout / IDL: [`idl/lp0002-msig.idl.json`](idl/lp0002-msig.idl.json)
- Benchmarks (proving times): [`docs/lp0002-benchmarks.md`](docs/lp0002-benchmarks.md)
- Reliability / failure modes: [`docs/lp0002-reliability.md`](docs/lp0002-reliability.md)
- CI for the msig paths: [`.github/workflows/lp0002-ci.yml`](.github/workflows/lp0002-ci.yml)

The original upstream LEZ README continues below in [`README.md`](README.md).
