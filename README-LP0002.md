# LP-0002: Anonymous M-of-N Multisig

> **v0.2.0-rc5 port:** this submission is now built on Logos LEZ **v0.2.0-rc5** and adds an
> in-circuit live-account binding (review item #6). The end-to-end demo is
> `scripts/lp0002-demo.sh` (local, green under `RISC0_DEV_MODE=1`); the live-testnet 2-of-3
> evidence was **captured on the redeployed rc5 testnet on 2026-06-26** (deploy tx `2262403372…`,
> two distinct vote nullifiers, treasury drained — see `docs/LP-0002-solution.md`).


This is an LP-0002 solution built as a fork of Logos **LEZ v0.2.0-rc5** (the upstream Logos
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

Everything outside the paths below is upstream Logos LEZ v0.2.0-rc5, unchanged. See `NOTICE`
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
- msig tests in `lee/state_machine/src/state.rs` (public-tx + bootstrap + compose) and
  `lee/state_machine/src/privacy_preserving_transaction/circuit.rs` (approve tests, including one real
  `RISC0_DEV_MODE=0` STARK plus negatives).
- LP-0002 packaging: this file, `NOTICE`, `scripts/lp0002-demo.sh`,
  `docs/LP-0002-solution.md`, `docs/lp0002-benchmarks.md`, `docs/lp0002-reliability.md`,
  `idl/lp0002-msig.idl.json`, `.github/workflows/lp0002-ci.yml`.

## Prerequisites

This is a fork of Logos LEZ v0.2.0-rc5, so it builds like upstream LEZ. You need the Rust
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

The end-to-end demo also needs the `logos-blockchain-circuits` release (a separate Logos
artifact that `rzup` does not install) at `~/.logos-blockchain-circuits`. `scripts/lp0002-demo.sh`
auto-downloads the pinned `v0.4.2` if it is absent, so a fresh `./demo.sh` is turnkey. To install
it manually:

```sh
mkdir -p ~/.logos-blockchain-circuits
curl -sSL https://github.com/logos-blockchain/logos-blockchain-circuits/releases/download/v0.4.2/logos-blockchain-circuits-v0.4.2-linux-x86_64.tar.gz \
  | tar -xz --strip-components=1 -C ~/.logos-blockchain-circuits
```

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
> local standalone sequencer, prefer `scripts/lp0002-demo.sh` (or `DEV_MODE=1 scripts/lp0002-demo.sh`
> for a fast no-proof plumbing check), which wires all of the above together and asserts the result.

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

## Deployed program

- Network: `testnet.lez.logos.co`
- Program id (base58): `9pwpqhXCZqzBDYctvTvzPeV1qoviSAENw2utmayHgvBF` (rc5; decimal `[3100124547, …]`). **Live deploy/evidence: ✅ captured 2026-06-26** — deploy tx `2262403372e8681604ce330f0040a1680b89f7db1c622ad6087e2bcf92fe8892`; the live proposal account is owned by this program id
- Program id (8x u32 le): `[3100124547, 2797454125, 2467287583, 3014535533, 2620419628, 3253148841, 840948196, 515808628]`

## Live on-chain evidence

The full evidence record, with transaction hashes and proving times, is in
[`docs/LP-0002-solution.md`](docs/LP-0002-solution.md). In short:

- **2-of-3 threshold** (the M-of-N proof, HD-nsk-derived membership): proposal
  `Hf84MVjYamaaCxmBpziYEow6JNuLH7SBNdzLwArf23vu` (member_root `fe674331`, three HD-derived
  shielded-account members), two anonymous approvals from two distinct members
  (`2614f4a9` count 0 -> 1, `09f00672` count 1 -> 2) with two **distinct** proposal-bound vote
  nullifiers (`a139609a`, `0e491ba7`), then InitTreasury (`d397291b` / `4f191345`), fund 100
  (`c851e0e4`), and execute at threshold=2 (`2354ebbd`, treasury 100 -> 0, recipient 0 -> 100).
  Deploy tx `2262403372`. Every approve is a real `RISC0_DEV_MODE=0` STARK (inner-guest ≈30 s +
  outer-circuit ≈151 s, succinct proof ≈229 KB); any hash is verifiable via
  `wallet chain-info transaction --hash <hash>`.

## Further reading

- Architecture map (component map + flow diagram): [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md)
- Instruction layout / IDL: [`idl/lp0002-msig.idl.json`](idl/lp0002-msig.idl.json)
- Benchmarks (proving times): [`docs/lp0002-benchmarks.md`](docs/lp0002-benchmarks.md)
- Reliability / failure modes: [`docs/lp0002-reliability.md`](docs/lp0002-reliability.md)
- CI for the msig paths: [`.github/workflows/lp0002-ci.yml`](.github/workflows/lp0002-ci.yml)

The original upstream LEZ README continues below in [`README.md`](README.md).
