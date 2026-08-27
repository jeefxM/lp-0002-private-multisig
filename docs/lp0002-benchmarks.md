# LP-0002 msig: Benchmarks (compute, cost, performance)

This document reports the performance and cost profile of the LP-0002 anonymous
M-of-N multisig (`msig`) program on the Logos LEZ v0.2.0-rc5 testnet rev
(`testnet.lez.logos.co`, program `9pwpqhXCZqzBDYctvTvzPeV1qoviSAENw2utmayHgvBF`).

It is written to be honest about what this rev does and does not expose. The
headline fact: **this Logos LEZ v0.2.0-rc5 rev has no compute-unit / gas / fee field**, so
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

- **`lez/common/src/transaction.rs`**, the `LeeTransaction` enum and its
  `transaction_stateless_check` / `validate_on_state` / `execute_check_on_state`
  paths contain no gas, fee, compute-unit, or priority field. A transaction is a
  `Public`, `PrivacyPreserving`, or `ProgramDeployment` variant; none carries a
  cost field.
- **Wallet chain CLI (`lez/wallet/src/cli/chain.rs`)**, the `ChainSubcommand` set is
  exactly `block-id`, `block`, and `tx`. Querying a transaction prints the full
  transaction via `{tx:#?}` (debug), which would surface any fee/gas field if one
  existed; there is none. Querying a block likewise prints the full block with no
  cost accounting.
- **Wallet account CLI (`lez/wallet/src/cli/account.rs`)**, account state is
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

**Real-proof approve time: ~180 s** to generate one DEV_MODE=0 STARK on the
build box (AMD EPYC-Genoa, 16 cores). This is a local timing observation, not a
chain-reported figure.

This is corroborated by the canonical on-chain 2-of-3 run (proposal `Hf84MVjY`,
member_root `fe674331`), each approve of which required a local DEV_MODE=0 prove
before the resulting tx landed:

- approve #1 (member 0) `2614f4a9`, ~180 s, count 0 -> 1.
- approve #2 (member 1) `09f00672`, ~180 s, count 1 -> 2.

The two threshold approvals are separate ~180 s proves by two different members;
their vote nullifiers (`a139609a`, `0e491ba7`) are distinct, and the proposal
state stores only `root + id + count + opaque nullifiers`, no member identity. So
the per-approval cost scales linearly in the number of approvers (one ~180 s prove
each), and that linear cost is **serial, not parallel**: each approve commits the
full live ProposalState (count + nullifier set) into its proof, and apply rejects
a proof built against a now-stale snapshot (see reliability doc LP23,
`InconsistentAccountPreState`). Members may prove on independent machines, but only
one approval per proposal-state-version can land, a proof built before another
approval landed must be re-run against the updated state. In the canonical 2-of-3
run the two approvals landed sequentially (count 0 -> 1, then count 1 -> 2) with a
finality gate enforced between them; approve #2 was necessarily proved against the
count=1 state. Effective throughput is one ~180 s approval at a time.

For reference, the build-only path (no prove) and the public ops are sub-second;
the ~180 s is entirely the STARK.

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
(`2354ebbd` in the 2-of-3 run) carries only an
instruction (`Execute { threshold, seed }`), an account-id list, and no proof,
it is a tiny message by comparison. The privacy approve is the only "heavy" block
contributor in the whole flow; every other op is a small public message.

---

## 5. RISC0 cycle count for the approve guest: measured (live rc5 DEV_MODE=0)

The RISC0 cycle count (total / user cycles, segment count) of the `approve` guest
execution is the natural compute proxy, and the live rc5 DEV_MODE=0 run measured
it directly. The proving harness emits `MEASURE_INNER_GUEST` and
`MEASURE_OUTER_CIRCUIT` `SessionStats` lines (captured in `.testnet-demo/run.log`)
for both threshold approves:

1. **Inner approve guest** (the in-guest Merkle-membership check + proposal-bound
   nullifier derivation): **262,144 total cycles**, **197,041-209,217 user cycles**
   across the two approves, **1 segment**, **~30 s** to prove.
2. **Outer succinct circuit** (the recursion/wrap that yields the on-chain
   `InnerReceipt::Succinct`): **1,048,576 total cycles**, **~151 s** to prove.

Summed, those two stages are the **~180 s** wall per DEV_MODE=0 approve reported in
Sections 3 and 7: the inner guest is the cheap part and the outer succinct wrap
dominates. This rev still exposes no cycle count on-chain or in the wallet, so the
figures above come from instrumenting the prove run, not from a chain-reported
field. The counts are stable across both approves (262,144 inner / 1,048,576 outer
each), consistent with the fixed depth-5 circuit not varying with which member
proves.

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
| Approve real-proof time | ~180 s | live DEV_MODE=0 on AMD EPYC-Genoa, 16c; on-chain approves 2614f4a9 / 09f00672 |
| Approve receipt size | ~224 KB | on-chain receipt deserializes to `InnerReceipt::Succinct`; constant in member-set size at depth-5 |
| Approve cost scaling | linear, one ~180 s prove per approver; serialized through on-chain state (one approval per state-version lands) | 2-of-3 run = two distinct-member proves landing sequentially with a finality gate between them |
| Public-op cost (enroll/create/init/execute/fund) | sub-second RISC-V exec, no fee, no proof | Logos LEZ v0.2.0-rc5 public-tx path |
| Approve guest RISC0 cycle count | inner guest 262,144 total / 197,041-209,217 user, 1 segment; outer circuit 1,048,576 total | measured on live rc5 DEV_MODE=0 run (`.testnet-demo/run.log`, MEASURE_INNER_GUEST / MEASURE_OUTER_CIRCUIT) |

Bottom line: the only expensive thing in an LP-0002 multisig is generating each
member's anonymous-approval STARK (~180 s, ~224 KB, one per approver), and those
approvals are serialized through the proposal's on-chain state, one lands per
state-version, the next is proved against the updated count. Everything else is a
sub-second, fee-free public transaction. There is no gas or compute-unit price to
optimize on this rev because the rev does not have one.
