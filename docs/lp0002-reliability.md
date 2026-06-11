# LP-0002 msig — Reliability & Error Codes

This document covers the reliability surface of the LP-0002 anonymous M-of-N
multisig (`msig`) on nssa v0.1.2 (`testnet.lez.logos.co`, program
`HjHCub28GrUNgd2QuJ2SPob7YmaUgDRCGXwbt2jt4UWn`):

- **REL-3** — an enumerated error-code / failure table covering every reject,
  panic, and silent no-op path in the program, with each trigger and its
  member-facing meaning.
- **REL-1** — how proof generation / verification failures are handled and what a
  member sees, including a small code improvement to the approve runner.
- **REL-2** — partial-approval resumability, stated honestly.

There is no native error-code numbering in this nssa rev; the `LPxx` codes below
are assigned by this document for reference, scoped to the msig program's own
reject/panic/no-op paths plus the apply-layer rejections an msig transaction can
actually reach. They are not chain-level codes.

**Anonymity model (context for the approve-side codes).** The privacy property is
approver anonymity *within the enrolled set of N public members*: members enroll a
public leaf `H(secret)`, so the member set is public; on approval the **count is
public** but **which specific member approved is hidden** (the proposal stores only
`root + id + count + opaque nullifiers`, no member identity). This is anonymity
among public members, not hidden or anonymous membership. It is also why the
approve-side rejects below (LP01–LP04) can only say "you are not a member" or "you
already voted" without revealing who you are.

---

## REL-3: Error / failure table

The msig program rejects bad input through three distinct mechanisms, and a
fourth lives in the chain's apply layer. Understanding which mechanism fires is
the key to reading a failure:

1. **Guest `assert!` panic** — a logical precondition is violated. The guest
   panics; for a privacy (ZK) op this means the **proof cannot be generated** (the
   prove step fails); for a public op this means the **transaction is rejected at
   execution**. The assert message is the diagnostic.
2. **Guest `.expect()` panic** — a data-sizing invariant is violated (an account's
   `data` field would exceed its limit, or stored bytes are truncated). Same
   failure mode as an assert (panic during prove/exec), but indicates malformed or
   over-large state rather than a member mistake.
3. **Guest silent no-op (wrong arity)** — the instruction was handed the wrong
   number of pre-state accounts. The guest returns **without writing any
   `ProgramOutput`**. There is no panic and no state change; the transaction
   simply does nothing useful.
4. **Apply-layer rejection** — the guest produced a valid output, but the chain's
   state-transition validation rejects it (unauthorized claim, unauthorized
   balance decrease, stale pre-state, etc.). These are `InvalidProgramBehaviorError`
   / `ExecutionValidationError` variants raised at apply, not by the guest.

### Program-level failures (guest)

| Code | Trigger | Source | Mechanism | Member-facing meaning |
|------|---------|--------|-----------|-----------------------|
| LP01 | Approve: proposal `data` shorter than `PROPOSAL_HEADER_LEN` (68 B) | `msig.rs:82` `assert!(data.len() >= PROPOSAL_HEADER_LEN, "proposal state header too short")` | assert panic (prove fails) | The ProposalState you referenced is not a valid, created proposal (header missing). Create the proposal first. |
| LP02 | Approve: supplied `proposal_id` != the id frozen in the ProposalState | `msig.rs:87` `assert_eq!(proposal_id, proposal_id_state, "proposal id mismatch")` | assert panic (prove fails) | You are approving the wrong proposal id; it does not match the live proposal. |
| LP03 | Approve: approver's leaf `H(secret)` is not in the proposal's frozen `member_root` | `msig.rs:91` `assert_eq!(root_from_path(leaf, &merkle_path), member_root, "approver is not an enrolled member")` | assert panic (prove fails) | You are not an enrolled member of this proposal's member set (or your Merkle path is wrong). Only the public enrolled members can approve. |
| LP04 | Approve: this secret's proposal-bound nullifier is already recorded | `msig.rs:106` `assert!(!nullifiers.contains(&nullifier), "approval nullifier already recorded (double vote)")` | assert panic (prove fails) | You have already approved this proposal. One approval per member per proposal; no double votes. |
| LP05 | Execute: proposal `data` shorter than `PROPOSAL_HEADER_LEN` | `msig.rs:146` `assert!(data.len() >= PROPOSAL_HEADER_LEN, "proposal state header too short")` | assert panic (exec rejected) | The proposal account passed to execute is not a valid proposal. |
| LP06 | Execute: `approval_count < threshold` | `msig.rs:148` `assert!(count >= threshold, "approval count below threshold")` | assert panic (exec rejected) | Not enough members have approved yet; the treasury stays locked until count >= M. |
| LP07 | CreateProposal: claimed account's data would exceed the data limit | `msig.rs:32` `.expect("proposal state fits into data limit")` | expect panic | Internal sizing invariant; the proposal header always fits, so this indicates a corrupted/oversized input account. |
| LP08 | Enroll: a stored registry leaf is truncated when re-read | `msig.rs:48` `.expect("registry leaf truncated")` | expect panic | The MembersRegistry account data is malformed (truncated leaf). Indicates a corrupted registry, not normal use. |
| LP09 | Enroll: recomputed registry data would exceed the data limit | `msig.rs:63` `.expect("registry should fit into data limit")` | expect panic | Too many members for the account data limit (registry full / oversized). |
| LP10 | Approve: a stored nullifier is truncated when re-read | `msig.rs:103` `.expect("nullifier set truncated")` | expect panic | The ProposalState nullifier set is malformed (truncated). Indicates corrupted proposal state. |
| LP11 | Approve: recomputed proposal data would exceed the data limit | `msig.rs:124` `.expect("proposal state should fit into data limit")` | expect panic | Too many recorded approvals for the data limit (proposal full). |
| LP12 | Any instruction: wrong number of pre-state accounts (CreateProposal != 1, Approve != 2, Enroll != 1, InitTreasury != 1, Execute != 3) | `msig.rs:215/225/231/240/257` `let Ok([..]) = <[_; N]>::try_from(pre_states) else { return; }` | silent no-op (no ProgramOutput) | The transaction was built with the wrong account list; it does nothing and applies no state change. A client/runner construction bug, not a member action. |

Note on LP01–LP04 mechanics: because `Approve` is a privacy (ZK) transaction, all
four of these asserts fire **inside the guest during local proof generation**, so
the failure happens on the member's own machine **before any transaction is
submitted**. The on-chain proposal state is never touched by a failed approve.
This is why REL-1 (below) is about the local prove surface, not an on-chain
revert.

### Apply-layer failures (chain state transition)

These are not raised by the msig guest; they are raised by the chain's
state-transition validation when applying an otherwise-valid program output.
msig's bootstrap design exists specifically to avoid the first one.

| Code | Trigger | Variant | Member-facing meaning | Evidence |
|------|---------|---------|-----------------------|----------|
| LP20 | A fresh (default-owned) account is `Claim::Authorized` without a signer or caller-authorized PDA — e.g. a plain transfer to an uninitialized treasury PDA, which can never sign | `InvalidProgramBehaviorError::ClaimedUnauthorizedAccount` | You cannot fund a fresh treasury PDA with a plain transfer; the PDA holds no key and can never authorize its own claim. Use `InitTreasury` first (it claims the PDA under msig's PDA authorization), then a plain transfer funds the now-owned account. | Reproduced in-process by `msig_fund_treasury_pda_rejected` (nssa/src/state.rs): arms (a) and (c) both fail with `ClaimedUnauthorizedAccount`; this is also why `run_init_treasury` exists. |
| LP21 | A signer-required claim is attempted with no signer — e.g. enroll built with an empty signer list against an unsigned registry | `InvalidProgramBehaviorError::ClaimedUnauthorizedAccount` | The registry claim needs the registry keypair's signature. An enroll with no signer is rejected; sign each `Enroll` with the registry key. | Reproduced in-process by `msig_enroll_public_tx_apply_rejection` (nssa/src/state.rs): a no-signer enroll rejects at apply; the working path signs with the registry keypair (`msig_enroll_signer_owned_appends`). |
| LP22 | Claiming an account that is not default (already initialized) | `InvalidProgramBehaviorError::ClaimedNonDefaultAccount` | You tried to create/claim a proposal (or treasury) account id that already exists. Use a fresh id, or reference the existing one instead of re-creating it. | `create_proposal` claims a fresh account; re-running it on the same id rejects. |
| LP23 | The pre-state fed into the proof/exec does not match live on-chain state (e.g. a stale nonce, or `is_authorized` mismatched against the live reconstruction) | `InvalidProgramBehaviorError::InconsistentAccountPreState` / `InconsistentAccountAuthorization` | The proposal state changed (or your `is_authorized` flag was wrong) between reading it and submitting; re-read the live account and rebuild. This is exactly why `run_approve` reads the live ProposalState (owner/balance/data/nonce) before proving. | The `is_authorized = false` requirement is validated by `msig_approve_live_apply_is_authorized_false` (nssa/src/state.rs). |
| LP24 | A program tries to decrease the balance of an account it does not own | `ExecutionValidationError::UnauthorizedBalanceDecrease` | A balance can only be debited by its owning program. The treasury must be `authenticated_transfer`-owned (via `InitTreasury`) so the chained `Execute` drain is authorized. | `ExecutionValidationError::UnauthorizedBalanceDecrease` (nssa/core/src/program.rs); covered by the program-owner balance tests in nssa/src/state.rs. |

### Failure-count summary

- **Guest program-level codes (LP01–LP12): 12.** 6 `assert!` (LP01–LP06), 5
  `.expect()` data-sizing (LP07–LP11), 1 wrong-arity silent no-op class (LP12,
  covering all five instruction arms).
- **Apply-layer codes msig can reach (LP20–LP24): 5.**
- **Total documented failure codes: 17** (scope: the msig guest's own
  reject/panic/no-op paths plus the apply-layer rejections an msig transaction can
  actually trigger).

---

## REL-1: Proof-failure handling and the member-facing error surface

There are **two distinct failure surfaces** for an approval, and they fail very
differently. Both are documented here honestly.

### Surface (a): local proof fails to generate

This is the common case. Every approve-side reject (LP01–LP04) is a guest
`assert!` that panics **inside the inner program prove**, before any submission.
In the code, `execute_and_prove` → `execute_and_prove_program` runs the guest
prover and maps a guest panic to
`NssaError::ProgramProveFailed(e.to_string())` (nssa/src/privacy_preserving_transaction/circuit.rs).
The raw string is an opaque RISC0 panic dump. Concretely, a non-member approval
surfaces as a 32-byte hash mismatch dump
(`assertion left == right failed: approver is not an enrolled member`, with both
roots printed) and a double vote surfaces as
`approval nullifier already recorded (double vote)` — both buried in a prover
backtrace. A member cannot reasonably read that.

**Code improvement made (small, REL-1):** `examples/program_deployment/src/bin/run_approve.rs`
previously wrapped the prove failure as the bare
`anyhow!("execute_and_prove failed: {e}")`. It now wraps it in a clear
member-facing message that enumerates the only three conditions that can reject an
approve, then attaches the raw prover error for operators:

> approval proof could not be generated. The approve guest rejected this attempt;
> the cause is one of: (1) you are not an enrolled member of this proposal's
> frozen member set, (2) you have already approved this proposal (your
> proposal-bound vote nullifier is already recorded — no double votes), or (3) the
> proposal id / member root you supplied does not match the live ProposalState.
> Nothing was submitted, so the on-chain approval count is unchanged; fix the
> input and re-run. Raw prover error: {e}

This deliberately does **not** claim to classify which assert fired: the RISC0
error string is not reliably machine-parseable, so the message lists the full set
of approve-side rejects rather than guessing. The original file is backed up to
`/tmp/run_approve.rs.bak-*` before editing, and the runner still compiles
(`cargo build --release -p program_deployment --bin run_approve`).

### Surface (b): proof verifies locally but the tx is rejected at apply

A proof can generate fine yet still be rejected when the sequencer applies it —
the apply-layer codes LP23 (stale pre-state / `is_authorized` mismatch), LP22, or
a malformed message. Today the runner gets a `tx_hash` back from
`send_transaction` and exits successfully; if the chain later drops the tx at
apply, the **approval count never increments and the member sees no error** —
they have a tx hash but no effect. `run_approve` already mitigates the most common
cause (it reads the live ProposalState so the proof's pre-state and nonce match
what landed), but it does not confirm the effect.

**Recommended clean surface for (b) (documented, not implemented):** after
`send_transaction` returns, poll the live ProposalState and confirm
`approval_count` increased (and that your vote nullifier is now present). If it did
not increase within a few blocks, report "approval was submitted but did not
apply — re-read the proposal and retry" rather than treating the returned
`tx_hash` as success. This is a post-submit verification step, beyond the scope of
the small message change above, so it is recommended here rather than coded.

---

## REL-2: Partial-approval resumability (honest framing)

Partial-approval resumability is **partly durable and partly not**, and the line
between them is the important reliability fact:

- **What resumes: the on-chain approval state is durable.** Each successful
  `Approve` increments `approval_count` and appends the member's vote nullifier to
  the ProposalState on-chain. This survives client restarts, crashes, and
  disconnects completely. A 2-of-3 proposal that has one approval recorded
  on-chain still shows count 1 to every client afterward; the second member's
  later approval picks up from count 1 and reaches the threshold. The proposal
  state is the single source of truth and it persists independently of any client.
- **What does NOT resume: in-progress local proving has no client-side
  save/load.** A single `Approve` requires a ~133 s local STARK. If that prove is
  interrupted (the client is killed mid-proof), there is no checkpoint of the
  partial proof; the interrupted prove must be re-run from scratch. The
  `run_approve` runner has no resume/checkpoint of an in-flight proof, and it does
  not persist any in-progress proving artifact between invocations.

So: **approvals already recorded on-chain resume automatically (the on-chain count
is durable); an interrupted local proof does not resume and must be re-run.
Client-side resume of in-progress proving is a known limitation, not an
implemented feature.**

Because a failed or interrupted approve submits nothing (Surface (a) above), re-
running it is always safe: it cannot double-count (the nullifier check rejects a
genuine second vote), and it cannot corrupt the durable on-chain count.
