# LP-0002 msig: Reliability & Error Codes (LEZ v0.2.4)

This document covers the reliability surface of the LP-0002 anonymous M-of-N
multisig (`msig`) as ported to **Logos LEZ v0.2.4**. It is adapted from the
original v0.1.2 reliability note (`/root/lez-v012/docs/lp0002-reliability.md`);
the program logic is the same ported guest, but the chain target, program id, and
source paths differ and have been updated here so the references match v0.2.4.

- **Target:** a **LOCAL standalone sequencer** (`sequencer_service` built with the
  `standalone` feature, `RISC0_DEV_MODE=1`). The local resume/error demos in this note run against this local sequencer,
  but the canonical real-STARK evidence run **did** land on
  `testnet.lez.logos.co` (the v0.2.4 2-of-3 proposal
  `Hf84MVjYamaaCxmBpziYEow6JNuLH7SBNdzLwArf23vu`, `approval_count=2`, executed).
- **Program id (v0.2.4):** `MSIG_ID` (8 × u32 LE) =
  `[1155063609, 1918607948, 1043343914, 2266441241, 1831314946, 53341822, 1565811176, 2148869898]`,
  i.e. base58 program id `4tvD5XPFc4ofgN3YV4ymZ1nWqn3iUwB9tucesYzBKJB9` (32-byte
  hex `8325c8b82dc3bda61fd20f936d29aeb32c6e309ca91ce7c1e4d91f32749dbe1e`).
- **Guest source:** `lee/state_machine/test_methods/guest/src/bin/msig.rs` (line numbers below
  are verified against this v0.2.4 file).

It covers:

- **REL-3**, an enumerated error-code / failure table covering every reject,
  panic, and silent no-op path in the program, with each trigger and its
  member-facing meaning.
- **REL-1**, how proof generation / verification failures are handled and what a
  member sees.
- **REL-2**, partial-approval resumability — **now concretely demonstrated** by
  `scripts/lp0002-resume.sh` (see below).

There is no native error-code numbering in this rev; the `LPxx` codes below are
assigned by this document for reference, scoped to the msig program's own
reject/panic/no-op paths plus the apply-layer rejections an msig transaction can
actually reach. They are not chain-level codes.

**Anonymity model (context for the approve-side codes).** The privacy property is
approver anonymity *within the enrolled set of N public members*: members enroll a
public leaf `H(secret)`, so the member set is public; on approval the **count is
public** but **which specific member approved is hidden** (the ProposalState stores
only `root + id + count + opaque nullifiers`, no member identity). This is anonymity
among public members, not hidden or anonymous membership. It is also why the
approve-side rejects below (LP01–LP04) can only say "you are not a member" or "you
already voted" without revealing who you are.

**Review item #6 (live-account binding).** The approve guest
additionally binds each anonymous approval to the member's **live, funded shielded
voting account** keyed by the same `secret` as the membership leaf (two extra
asserts, LP01a / LP01b below). This prevents an approval that does not ride a real,
owned member account; it does not weaken anonymity (the rider is the member's own
private account, and only the count is revealed).

---

## REL-3: Error / failure table

The msig program rejects bad input through three distinct mechanisms, and a fourth
lives in the chain's apply layer:

1. **Guest `assert!` panic** — a logical precondition is violated. The guest panics;
   for a privacy (ZK) op this means the **proof cannot be generated** (the prove step
   fails); for a public op this means the **transaction is rejected at execution**.
2. **Guest `.expect()` panic** — a data-sizing invariant is violated (an account's
   `data` field would exceed its limit, or stored bytes are truncated). Same failure
   mode as an assert, but indicates malformed/over-large state rather than a member
   mistake.
3. **Guest silent no-op (wrong arity)** — the instruction was handed the wrong number
   of pre-state accounts; the guest returns **without writing any `ProgramOutput`**.
   No panic, no state change.
4. **Apply-layer rejection** — the guest produced a valid output, but the chain's
   state-transition validation rejects it (unauthorized claim, unauthorized balance
   decrease, stale pre-state, etc.).

### Program-level failures (guest) — `lee/state_machine/test_methods/guest/src/bin/msig.rs`

| Code | Trigger | Source (v0.2.4 line) | Mechanism | Member-facing meaning |
|------|---------|-------------------|-----------|-----------------------|
| LP01 | Approve: proposal `data` shorter than `PROPOSAL_HEADER_LEN` (68 B) | `msig.rs:89` `assert!(data.len() >= PROPOSAL_HEADER_LEN, "proposal state header too short")` | assert panic (prove fails) | The ProposalState you referenced is not a valid, created proposal. Create the proposal first. |
| LP02 | Approve: supplied `proposal_id` != id frozen in the ProposalState | `msig.rs:94` `assert_eq!(proposal_id, proposal_id_state, "proposal id mismatch")` | assert panic (prove fails) | You are approving the wrong proposal id; it does not match the live proposal. |
| LP01a | Approve: rider account id != `for_regular_private_account(npk(secret), vpk, VOTE_IDENTIFIER)` (the member's ML-KEM vpk travels in the private witness; ids are vpk-bound since v0.2.1) (review item #6) | `msig.rs:102` `assert_eq!(rider.account_id, expected_rider, "rider must be the member's account keyed by the voting secret")` | assert panic (prove fails) | The account you rode the approval on is not your voting account derived from your secret. |
| LP01b | Approve: rider account is `Account::default()` (fresh/uninitialized) (review item #6) | `msig.rs:106` `assert_ne!(rider.account, Account::default(), "rider must be a LIVE funded account, not a fresh init")` | assert panic (prove fails) | Your voting account is not live/funded yet. Fund it (a shielded transfer) before approving. |
| LP03 | Approve: approver's leaf `H(secret)` is not in the proposal's frozen `member_root` | `msig.rs:117` `assert_eq!(root_from_path(leaf, &merkle_path), member_root, "approver is not an enrolled member")` | assert panic (prove fails) | You are not an enrolled member of this proposal's member set (or your Merkle path is wrong). |
| LP04 | Approve: this secret's proposal-bound nullifier is already recorded | `msig.rs:131` `assert!(!nullifiers.contains(&nullifier), "approval nullifier already recorded (double vote)")` | assert panic (prove fails) | You have already approved this proposal. One approval per member per proposal. |
| LP05 | Execute: proposal `data` shorter than `PROPOSAL_HEADER_LEN` | `msig.rs:168` `assert!(data.len() >= PROPOSAL_HEADER_LEN, "proposal state header too short")` | assert panic (exec rejected) | The proposal account passed to execute is not a valid proposal. |
| LP06 | Execute: `approval_count < threshold` | `msig.rs:170` `assert!(count >= threshold, "approval count below threshold")` | assert panic (exec rejected) | Not enough members have approved yet; the treasury stays locked until count >= M. |
| LP07 | CreateProposal: claimed account's data would exceed the data limit | `msig.rs:38` `.expect("proposal state fits into data limit")` | expect panic | Internal sizing invariant; indicates a corrupted/oversized input account. |
| LP08 | Enroll: a stored registry leaf is truncated when re-read | `msig.rs:54` `.expect("registry leaf truncated")` | expect panic | The MembersRegistry account data is malformed (truncated leaf). |
| LP09 | Enroll: recomputed registry data would exceed the data limit | `msig.rs:69` `.expect("registry should fit into data limit")` | expect panic | Too many members for the account data limit (registry full / oversized). |
| LP10 | Approve: a stored nullifier is truncated when re-read | `msig.rs:126` `.expect("nullifier set truncated")` | expect panic | The ProposalState nullifier set is malformed (truncated). |
| LP11 | Approve: recomputed proposal data would exceed the data limit | `msig.rs:147` `.expect("proposal state should fit into data limit")` | expect panic | Too many recorded approvals for the data limit (proposal full). |
| LP12 | Any instruction: wrong number of pre-state accounts | `msig.rs:236/247/253/262/279` `... else { return; }` | silent no-op (no ProgramOutput) | The transaction was built with the wrong account list; it does nothing and applies no state change. A client/runner construction bug, not a member action. |

Note on LP01–LP04 (and LP01a/LP01b) mechanics: because `Approve` is a privacy (ZK)
transaction, these asserts fire **inside the guest during local proof generation**,
so the failure happens on the member's own machine **before any transaction is
submitted**. The on-chain proposal state is never touched by a failed approve.

### Apply-layer failures (chain state transition)

These are raised by the chain's state-transition validation when applying an
otherwise-valid program output. msig's bootstrap design (`InitTreasury` before
funding) exists specifically to avoid the first one.

| Code | Trigger | Member-facing meaning |
|------|---------|-----------------------|
| LP20 | A fresh (default-owned) account is `Claim::Authorized` without a signer or caller-authorized PDA (e.g. a plain transfer to an uninitialized treasury PDA, which can never sign) | You cannot fund a fresh treasury PDA with a plain transfer; the PDA holds no key. Use `InitTreasury` first (it claims the PDA under msig's PDA authorization), then a plain transfer funds the now-owned account. |
| LP21 | A signer-required claim with no signer (e.g. enroll built with an empty signer list against an unsigned registry) | The registry claim needs the registry keypair's signature; sign each `Enroll` with the registry key. |
| LP22 | Claiming an account that is not default (already initialized) | You tried to create/claim a proposal (or treasury) id that already exists. Use a fresh id, or reference the existing one. |
| LP23 | Pre-state fed into the proof/exec does not match live on-chain state (stale nonce, `is_authorized` mismatch) | The proposal state changed between read and submit; re-read the live account and rebuild. This is why `run_approve` reads the live ProposalState before proving. |
| LP24 | A program tries to decrease the balance of an account it does not own | A balance can only be debited by its owning program; the treasury must be `authenticated_transfer`-owned (via `InitTreasury`) so the chained `Execute` drain is authorized. |

---

## REL-1: Proof-failure handling and the member-facing error surface

There are **two distinct failure surfaces** for an approval.

### Surface (a): local proof fails to generate

Every approve-side reject (LP01–LP04, plus the rider asserts LP01a/LP01b) is a
guest `assert!`/`assert_eq!` that panics **inside the inner program prove**, before
any submission. The prover panic is mapped to a `ProgramProveFailed`-class error
(in `lez/wallet/src/lib.rs`, the `execute_and_prove` path), surfaced as an
opaque RISC0 panic dump — e.g. a non-member approval prints
`assertion failed: approver is not an enrolled member` and a double vote prints
`approval nullifier already recorded (double vote)`, both buried in a prover
backtrace a member cannot reasonably read.

**Recommended (not yet in `run_approve`):** wrap the prove failure in a clear,
member-facing message that enumerates the only conditions that can reject an
approve — (1) you are not an enrolled member of this proposal's frozen member set,
(2) you have already approved (your proposal-bound vote nullifier is already
recorded; no double votes), (3) your voting-account rider is not your live funded
account keyed by your secret, or (4) the proposal id / member root you supplied does
not match the live ProposalState — then attach the raw prover error for operators.
Because nothing is submitted on a failed prove, re-running after fixing the input is
always safe. (The v0.1.2 note implemented this wrapper in its `run_approve`; the current
`run_approve` does not yet carry it, so it is documented here as a recommendation
rather than a claim.)

### Surface (b): proof verifies locally but the tx is rejected at apply

A proof can generate fine yet still be rejected at apply (LP23 stale pre-state /
`is_authorized` mismatch, LP22, or a malformed message). `run_approve` already
mitigates the most common cause (it reads the live ProposalState so the proof's
pre-state and nonce match what landed), but it returns a `tx_hash` without confirming
the effect.

**Recommended clean surface for (b):** after submitting, poll the live ProposalState
and confirm `approval_count` increased (and that your vote nullifier is now present);
if it did not within a few blocks, report "submitted but did not apply, re-read and
retry" rather than treating the returned `tx_hash` as success. `run_read_status`
(used heavily by the demo/resume scripts) is exactly the read-only poller this needs.

---

## REL-2: Partial-approval resumability — DEMONSTRATED

Partial-approval resumability is **partly durable and partly not**, and the line
between them is the important reliability fact. On this rev the durable half is no
longer just asserted — it is **demonstrated end-to-end** by
`scripts/lp0002-resume.sh`.

### What resumes: the on-chain approval state is durable (now demonstrated)

Each successful `Approve` increments `approval_count` and appends the member's vote
nullifier to the ProposalState. The sequencer persists that account state to RocksDB
**atomically on every block**:

- `sequencer/core` `produce_new_block()` builds the block, applies the validated
  state diff to its in-memory `state`, then synchronously calls
  `store.update(&block, …, &self.state)` → `dbio.atomic_update(block, …, state)` —
  an atomic RocksDB write of the block **and** the resulting Lee state. Because this
  runs under the sequencer lock inside `produce_new_block`, by the time the new
  count is observable over RPC it is **already persisted**; there is no window where
  `run_read_status` shows count==1 but disk does not.
- On startup, `start_from_config` does
  `if rocksdb_path.exists() { open_db; get_lee_state() } else { create_db_with_genesis }`.
  A restart against the **same data dir** therefore **reopens the persisted state**
  (logs `Block cache prepared`, **no** `starting from genesis`) and continues with
  `chain_height = latest_block_meta.id` — it does **not** re-genesis or reset.

**Demonstration (`scripts/lp0002-resume.sh`, run GREEN under `RISC0_DEV_MODE=1`):**
boots a local standalone sequencer on a stable data dir, deploys, enrolls 3 members,
creates the proposal, funds the voting accounts, then:

1. `approve(member 0)` → `approval_count` 0 → **1** (a partial 1-of-2).
2. **`kill -9` the sequencer process** (a hard crash), wait for the pid to die and the
   port to be released, then **restart `sequencer_service` against the SAME data dir**.
   The restart log shows `Block cache prepared` and **no** `starting from genesis`,
   proving it reopened the existing RocksDB rather than re-genesising it.
3. **Load-bearing assertion:** `run_read_status` after the restart still reports
   `approval_count==1` (full status line:
   `{"ready":true,...,"approval_count":1,"threshold":2}`). The partial approval
   survived the crash/restart. (`remaining = threshold − count = 2 − 1 = 1`.)
4. `approve(member 1)` resumes from the persisted count, 1 → **2** (threshold reached).
5. `init_treasury` → fund treasury → `execute` → `run_assert_state` GREEN
   (`approval_count=2`, treasury drained to 0, recipient funded 5000).

So a 2-of-3 proposal with one approval recorded still shows count 1 to every client
after a sequencer restart, and the second member's later approval picks up from
count 1 and reaches the threshold. The ProposalState is the single source of truth
and persists independently of any client and across sequencer restarts.

### What does NOT resume: in-progress local proving has no client-side save/load

A single `Approve` requires a local STARK (slow outside dev mode). If that prove is
interrupted (the client is killed mid-proof), there is no checkpoint of the partial
proof; the interrupted prove must be re-run from scratch. `run_approve` has no
resume/checkpoint of an in-flight proof and persists no in-progress proving artifact
between invocations.

### Summary

**Approvals already recorded on-chain resume automatically (the on-chain count is
durable across sequencer kill+restart on the same data dir — demonstrated by
`scripts/lp0002-resume.sh`); an interrupted local proof does not resume and must
be re-run.** Client-side resume of in-progress proving is a known limitation, not an
implemented feature.

Because a failed or interrupted approve submits nothing (Surface (a) above),
re-running it is always safe: it cannot double-count (the nullifier check rejects a
genuine second vote) and it cannot corrupt the durable on-chain count.
