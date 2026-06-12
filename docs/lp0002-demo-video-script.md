# LP-0002 Demo Video Script: Private M-of-N Multisig

Target length: 6 to 8 minutes. Segment-cut (record each segment, trim the dead time during proofs, stitch). Narration is plain and honest. Keep `RISC0_DEV_MODE=0` visible on screen during the real proof so the dev-mode question never comes up.

Canonical facts to keep on hand (cite only these):
- Program id: `HjHCub28GrUNgd2QuJ2SPob7YmaUgDRCGXwbt2jt4UWn`
- Live 2-of-3 run: proposal `BZ182CU`; approve#1 `1bef810a` (block 49442, nullifier `cdda374f`, 133.49s); approve#2 `05a784ea` (block 49456, nullifier `3979979b`, 133.60s); execute `81c7e42c` (block 49458, treasury 20 to 0, recipient 100 to 120).
- Live 1-of-N e2e: approve `13f1f0c2` (block 49316), execute `2d07a56a` (block 49319).

---

## Segment 1: What this is (about 30s)

[SCREEN] Title card: "LP-0002: Private M-of-N Multisig on LEZ". Then the lambda-prize LP-0002 spec page.

[NARRATION]
"This is my solution to LP-0002, a private M-of-N multisig for the Logos Execution Zone. A normal multisig reveals who approved. This one lets a group prove that M of N members approved a proposal, and release funds, without revealing which members approved. It runs on the live LEZ testnet, with real zero-knowledge proofs."

---

## Segment 2: How it works (about 60s)

[SCREEN] A simple diagram or the `docs/LP-0002-solution.md` "threshold scheme" and "nullifier design" sections.

[NARRATION]
"The design is Semaphore-style. Each member enrolls a public leaf, the SHA-256 of a domain tag and their secret, into an on-chain Merkle registry. The member set is public, but the secrets are not. To approve a proposal, a member generates a zero-knowledge proof that their leaf is in the proposal's frozen member root, and records a nullifier bound to that proposal, the hash of their secret and the proposal id. The proof shows they are one of the enrolled members without revealing which one, and the nullifier stops anyone from voting twice on the same proposal. When the approval count reaches the threshold M, an execute step releases the treasury through a chained call. So anonymity is among the public enrolled set: the count is public, but which member approved stays hidden."

---

## Segment 3: The live on-chain proof (about 2.5 to 3 min), the core

[SCREEN] A terminal on the build box. Run the reproducible demo against a local sequencer at real proof mode. Have `RISC0_DEV_MODE=0` visible.

[ACTION] Run the packaged demo:
```
cd /root/lez-v012
RISC0_DEV_MODE=0 scripts/lp0002-demo.sh
```

[NARRATION] (over the run, cutting the two ~134s proofs)
"Here is the full flow, from a clean clone, against a local LEZ sequencer, at RISC0_DEV_MODE=0, so these are real STARK proofs, not dev-mode fakes. First it deploys the multisig program, enrolls three members, and creates a proposal with threshold two."

[ACTION] When the first approve starts, point at the prove line. [CUT] during the ~134s prove, resume when it returns a tx hash.

[NARRATION]
"Now member zero approves. This is a real proof and takes about two minutes. It lands, and the on-chain approval count goes from zero to one. Then member one approves, with a different secret, so a different nullifier. The count goes to two."

[ACTION] When execute runs:

[NARRATION]
"Two of three is met, so execute releases the treasury to the recipient. The assertions at the end confirm the on-chain state: count two, treasury drained, recipient credited."

[SCREEN] Optionally cut to the live testnet evidence in `docs/LP-0002-solution.md`:

[NARRATION]
"The same flow is already recorded on the public testnet under program id HjHCub28. The two approvals are transactions 1bef810a and 05a784ea, in blocks 49442 and 49456, with two distinct nullifiers, and the execute is 81c7e42c. The proposal state on-chain stores only the count and the opaque nullifiers. It never records which member voted."

---

## Segment 4: The Basecamp app (about 90s), the differentiator

[SCREEN] The Basecamp desktop app with the "Private Multisig (LP-0002)" plugin loaded.

[NARRATION]
"The same primitive is wired into a Basecamp app. It is a native plugin that loads in Basecamp and casts a real vote, not a static placeholder."

[ACTION]
1. Section 1: paste a member secret, click Derive leaf. Show the leaf.
2. Section 2: Refresh status. Show the live proposal at, say, 0 of 2.
3. Section 3: click Cast anonymous vote.

[NARRATION]
"I paste my secret. The leaf is derived locally, here in the app. The secret is sent only to the local prover, never to the sequencer. I cast a vote. The app runs the real membership proof through a local sidecar and submits it. The status updates, the count goes up. A second member votes, and we reach the threshold. A non-member secret is rejected before any proof, and voting twice is rejected as a double vote."

(For the recording, dev-mode keeps this fast; mention that the real-proof path is the same and is what Segment 3 showed.)

---

## Segment 5: Reproducibility and tests (about 30s)

[SCREEN] The test run and CI file.

[ACTION]
```
cargo test -p nssa --release msig_
```

[NARRATION]
"The logic is covered by the test suite, which passes. The demo you just saw runs from a clean clone with one script, and CI runs the build, the lints, and the end-to-end flow against a standalone sequencer. The full write-up, the IDL, and the reliability and benchmark notes are in the repo."

---

## Segment 6: Wrap (about 30s)

[SCREEN] The success-criteria checklist in `docs/LP-0002-solution.md`.

[NARRATION]
"To summarize: a private M-of-N multisig, live on the LEZ testnet, with real anonymous approvals and a working Basecamp app. Anonymity is among a public member set, the proof realness is established by the on-chain receipt being a succinct STARK and by the local DEV_MODE=0 run, and the known limitations, like the public member set and the fixed tree depth, are documented honestly in the write-up. Thanks for watching."

---

## Recording notes
- Keep `RISC0_DEV_MODE=0` on screen during Segment 3. It is the single most important honesty signal.
- Cut the two ~134s proofs in Segment 3; do not speed-ramp the tx hashes or counts, show those at real speed.
- Use the Basecamp dev-mode for Segment 4 so it is snappy; state once that the real-proof path is identical.
- Avoid the word "membership anonymity" without "among the public enrolled set". Never say "hidden membership".
