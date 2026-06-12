# LP-0002 msig: Benchmarks (compute, cost, performance)

This document reports the performance and cost profile of the LP-0002 anonymous
M-of-N multisig (`msig`) program on the nssa v0.1.2 testnet rev
(`testnet.lez.logos.co`, program `HjHCub28GrUNgd2QuJ2SPob7YmaUgDRCGXwbt2jt4UWn`).

It is written to be honest about what this rev does and does not expose. The
headline fact: **this nssa v0.1.2 rev has no compute-unit / gas / fee field**, so
there is no native "CU" or "gas" number to report. The sections below document
that absence with file references, then give the defensible proxy metrics that
actually characterize the cost of the privacy approve (the one expensive
operation) versus the cheap public operations.

Measurement host (the "build box"): AMD EPYC-Genoa, 16 cores, 32 GiB RAM. All
local timing numbers below were observed on this host. Timing is sensitive to
CPU, core count, and load; treat the figures as order-of-magnitude, not a spec.

**Anonymity model (read before "approve" anywhere below).** The privacy property
here is approver anonymity *within the enrolled set of N public members*: each
member enrolls a public leaf `H(secret)` into the registry, so the member set
itself is public. When a member approves, the approval **count is public**, but
**which specific member approved is hidden**, the proposal state records only
`root + id + count + opaque proposal-bound nullifiers`, never any member identity.
This is anonymity among public members, not hidden or anonymous membership.
Wherever this document says "anonymous approval," it means exactly this. Witness
privacy (the member secret never being published) is structural, not test-backed:
the secret is carried as a private witness committed only to an inner
program-execution journal that the outer succinct proof folds in and never
publishes; the only thing committed on-chain is `PrivacyPreservingCircuitOutput`,
which contains no secret and no instruction data.

---

## 1. There is no compute-unit / gas / fee on this rev

The msig program, the wallet, and the chain RPC carry **no** notion of compute
units, gas, priority fees, or a per-transaction cost field. A transaction is
either valid (it applies) or it is not; there is no metered price.

Verified absent in:

- **`common/src/transaction.rs`**, the `NSSATransaction` enum and its
  `transaction_stateless_check` / `validate_on_state` / `execute_check_on_state`
  paths contain no gas, fee, compute-unit, or priority field. A transaction is a
  `Public`, `PrivacyPreserving`, or `ProgramDeployment` variant; none carries a
  cost field.
- **Wallet chain CLI (`wallet/src/cli/chain.rs`)**, the `ChainSubcommand` set is
  exactly `block-id`, `block`, and `tx`. Querying a transaction prints the full
  transaction via `{tx:#?}` (debug), which would surface any fee/gas field if one
  existed; there is none. Querying a block likewise prints the full block with no
  cost accounting.
- **Wallet account CLI (`wallet/src/cli/account.rs`)**, account state is
  `program_owner`, `balance`, `data`, `nonce`. No fee/gas balance, no compute
  budget.

Consequence: we **do not** and **cannot** quote a CU or gas number for any msig
instruction. Doing so would be fabrication. The rest of this document is the
defensible alternative.

---

## 2. The cost split: one expensive op, five cheap ops

The msig flow is one privacy-preserving (ZK) transaction surrounded by plain
public transactions:

| Op | Tx kind | Where the work is | Cost driver |
|----|---------|-------------------|-------------|
| `Enroll` | Public | RISC-V exec on the sequencer | negligible |
| `CreateProposal` | Public | RISC-V exec on the sequencer | negligible |
| `InitTreasury` | Public (+ chained call) | RISC-V exec on the sequencer | negligible |
| `Execute` | Public (+ chained call) | RISC-V exec on the sequencer | negligible |
| Fund (plain transfer) | Public | RISC-V exec on the sequencer | negligible |
| **`Approve`** | **Privacy-preserving (ZK)** | **client-side STARK prove** | **dominant** |

The five public ops are ordinary RISC-V execution on the sequencer. On this rev
they carry **no fee** and complete in the time it takes the sequencer to execute
the guest and apply the post-state, sub-second, dominated by block cadence, not
by any compute the client does. They are not the cost story.

**`Approve` is the entire cost story.** It is a privacy-preserving transaction:
the member's secret, Merkle membership path, and proposal id travel as a private
witness, and the client must locally generate a real STARK proving in-guest
Merkle membership against the frozen `member_root`, deriving a proposal-bound
vote nullifier, and rejecting double-votes, all before anything is submitted.
Everything below profiles `Approve`.

---

## 3. Approve proof: time (real DEV_MODE=0 proxy)

The defensible proxy for "how expensive is an anonymous approval" is the
wall-clock time to generate the real proof.

**Real-proof approve time: ~133–134 s** to generate one DEV_MODE=0 STARK on the
build box (AMD EPYC-Genoa, 16 cores). This is a local timing observation, not a
chain-reported figure.

This is corroborated by the canonical on-chain runs, each of which required a
local DEV_MODE=0 prove before the resulting tx landed:

- 1-of-N e2e: approve `13f1f0c2`, real DEV_MODE=0 STARK, ~134 s, landed at block
  49316 (count 0 -> 1).
- 2-of-3 threshold demo (the M-of-N proof):
  - approve #1 (member 0) `1bef810a`, 133.49 s, block 49442, count 0 -> 1.
  - approve #2 (member 1) `05a784ea`, 133.60 s, block 49456, count 1 -> 2.

The two threshold approvals are separate ~133 s proves by two different members;
their vote nullifiers (`cdda374f`, `3979979b`) are distinct, and the proposal
state stores only `root + id + count + opaque nullifiers`, no member identity. So
the per-approval cost scales linearly in the number of approvers (one ~133 s prove
each), and that linear cost is **serial, not parallel**: each approve commits the
full live ProposalState (count + nullifier set) into its proof, and apply rejects
a proof built against a now-stale snapshot (see reliability doc LP23,
`InconsistentAccountPreState`). Members may prove on independent machines, but only
one approval per proposal-state-version can land, a proof built before another
approval landed must be re-run against the updated state. In the canonical 2-of-3
demo the two approvals landed sequentially (block 49442, count 0 -> 1; then block
49456, count 1 -> 2), 14 blocks apart; approve #2 was necessarily proved against
the count=1 state. Effective throughput is one ~133 s approval at a time.

For reference, the build-only path (no prove) and the public ops are sub-second;
the ~133 s is entirely the STARK.

---

## 4. Approve proof: size (receipt / proof bytes proxy)

The second defensible proxy is the proof artifact size.

The on-chain approve receipt deserializes to `InnerReceipt::Succinct`, a real
succinct STARK, **~224 KB**. This is categorically not a `Fake` receipt (a
dev-mode placeholder), and it is the object the privacy tx carries and the
sequencer folds in. The succinct receipt size is essentially constant in the
member-set size at this depth (the circuit is fixed depth-5, 32 member slots), so
the ~224 KB does not grow with the number of enrolled members.

Block-size contrast (qualitative, since no byte-cost field exists): the
`Approve` privacy transaction carries this ~224 KB succinct proof plus the
private rider's commitment/nullifier/ciphertext, whereas the public `Execute` tx
(`2d07a56a` in the 1-of-N run; `81c7e42c` in the 2-of-3 run) carries only an
instruction (`Execute { threshold, seed }`), an account-id list, and no proof,
it is a tiny message by comparison. The privacy approve is the only "heavy" block
contributor in the whole flow; every other op is a small public message.

---

## 5. RISC0 cycle count for the approve guest: not readily measured

A natural compute proxy would be the RISC0 cycle count (total / user cycles,
segment count) of the `approve` guest execution. **We do not report one**, for two
reasons:

1. This rev exposes no cycle count on-chain or in the wallet; obtaining it
   requires instrumenting a local prove run (`RUST_LOG=risc0_zkvm=info`) of the
   approve guest specifically.
2. A clean measurement would require a fresh DEV_MODE=0 prove (~133 s) plus the
   build artifacts, and the build host is disk-constrained. We deliberately did
   not trigger that run for a single proxy number.

Important integrity note: an unrelated guest from a different program happens to
have a logged cycle count on this host. **It is NOT the msig approve guest and is
not cited here**, any cycle figure on this box belongs to other work and must not
be read as an approve metric. The msig approve cycle count is simply not measured;
the time (~133 s) and size (~224 KB) proxies above stand in for it.

---

## 6. What "cheap" means for the public ops

`Enroll`, `CreateProposal`, `InitTreasury`, and `Execute` are public
transactions executed as RISC-V on the sequencer. They have:

- no client-side proving (no STARK; the public-execution path runs the guest and
  validates the post-state),
- no fee on this rev (Section 1),
- no proof artifact (small messages),
- ordinary apply latency bounded by block cadence.

`InitTreasury` and `Execute` each additionally emit one chained call (to
`authenticated_transfer`), which is more RISC-V execution on the sequencer, still
with no fee. The depth is bounded (`MAX_NUMBER_CHAINED_CALLS`); msig uses a single
chained call per op, well within that bound.

---

## 7. Summary table

| Metric | Value | Source / caveat |
|--------|-------|-----------------|
| CU / gas / fee per tx | **none exists** | `common/src/transaction.rs`, wallet chain/account CLI, verified absent |
| Approve real-proof time | ~133–134 s | local DEV_MODE=0 on AMD EPYC-Genoa, 16c; on-chain approves 13f1f0c2 / 1bef810a (133.49s) / 05a784ea (133.60s) |
| Approve receipt size | ~224 KB | on-chain receipt deserializes to `InnerReceipt::Succinct`; constant in member-set size at depth-5 |
| Approve cost scaling | linear, one ~133 s prove per approver; serialized through on-chain state (one approval per state-version lands) | 2-of-3 demo = two distinct-member proves landing sequentially (blocks 49442 -> 49456) |
| Public-op cost (enroll/create/init/execute/fund) | sub-second RISC-V exec, no fee, no proof | nssa v0.1.2 public-tx path |
| Approve guest RISC0 cycle count | not measured | requires a fresh local prove; not obtained, and no figure from other guests on this host is cited |

Bottom line: the only expensive thing in an LP-0002 multisig is generating each
member's anonymous-approval STARK (~133 s, ~224 KB, one per approver), and those
approvals are serialized through the proposal's on-chain state, one lands per
state-version, the next is proved against the updated count. Everything else is a
sub-second, fee-free public transaction. There is no gas or compute-unit price to
optimize on this rev because the rev does not have one.
