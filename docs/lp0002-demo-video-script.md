# LP-0002 demo video - script (short cut, read aloud)

> Just read this. Casual voice. If you flub a line, keep going and stitch in post.
> Target ~6 minutes.
>
> Honest spine, keep these three beats distinct: the **GUI casts a real anonymous
> vote** with a real STARK; **demo.sh** runs the whole lifecycle end to end at
> `RISC0_DEV_MODE=0`; the **on-chain read-back** proves it independently on the
> public testnet. Honest lines to keep all the way through: each member's secret is
> their **real shielded-account key**, so members hold shielded accounts (bound by
> derivation); the **member set is public** (what's hidden is which member approved,
> not the membership); and `RISC0_DEV_MODE=0` means **real proofs**.

## Pre-flight (before you record)

Bring the local `DEV_MODE=0` stack up from the **`lp0002-nsk-binding` branch**, not
an old build: sequencer + sidecar + Basecamp GUI on VNC, with the rebuilt plugin. On
this branch the three members are real HD-derived shielded-account keys, so the
enrolled `member_root` is `38ea719c...`. Confirm:

```bash
ssh hetzner
cd /root/lez-v012 && git rev-parse --abbrev-ref HEAD   # -> lp0002-nsk-binding
curl -s 127.0.0.1:8799/status                          # threshold 2; reset to count 0 for the take
```

- **GUI:** VNC up (`ssh -N -L 5900:127.0.0.1:5900 hetzner`, Screen Sharing to
  `127.0.0.1:5900`). Pick the window **"Private Multisig: anonymous M-of-N vote"**.
- **Dress-rehearse one vote** to learn the timing (a real vote is ~2-3 min), then
  reset to a clean 0/2 for the real take.
- Have `docs/LP-0002-solution.md` open on a second desktop for the architecture beat
  (also `docs/ARCHITECTURE.md` for the diagram).
- **Member secrets (these ARE the real HD-derived shielded-account `nsk`s, 64 hex):**
  - member 0 = `72d2833e462b171ea3ad2676b9967703c9a8620dabf49883986f3f36377bdc65` (derives leaf `50811a77...`)
  - member 1 = `eee2d6cf8978e0a0b67c03eb68f4c48c0d0f1cc3cf77ca9f671ad193a893720a` (leaf `05318633...`)
  - member 2 = `365434a89e065ad81a73062db4b6d7fe28b45e3a17fc9858fdb9ea4b8b0ab139` (leaf `aa2db57d...`)
  - Sanity check: paste member 0 and click Derive leaf, it should show `50811a77...`.
    If a vote is rejected as a non-member, the stack is on an old enrollment, so
    re-enroll from this branch.

Layout: VNC left, Mac terminal (SSH'd to Hetzner) right, face cam in a corner.

---

## Segment 1 - intro (~60 sec)

**On screen:** face cam, then cut to the app's empty state.

> "Hey, I'm Davit. I've been a fullstack developer for about four years, and I've
> shipped on the Logos stack before. At the Zuitzerland hackathon in 2025 I built
> on this same stack and won a prize. So I'm not new here, and I'm back for the LPrize."

> "This is my LP-0002 submission, the private M-of-N multisig. It's a multisig
> where a group can prove enough of them approved something and release funds,
> without revealing which members approved. A normal multisig shows you every
> signer. This one shows you the count, but not the names."

> "I built two things. A program-agnostic core library that does the membership
> and anonymous-vote crypto, so any LEZ program can reuse it, and a reference
> Basecamp app on top. The real vote runs through that same library."

> "One honest flag up front: what's hidden here is which member approved, not who
> the group is. The member list is a public set. The vote is the private part. So
> it's anonymity within a known group. I'll show you that on chain later, and I'll
> explain how members tie to real shielded accounts in a second."

---

## Segment 2 - architecture & key decisions (~100 sec)

> The spec asks the video to walk through the architecture and the key decisions.
> This is that beat. Show the design section of `docs/LP-0002-solution.md`, or
> `docs/ARCHITECTURE.md` for the diagram, and talk over it.

**On screen:** `LP-0002-solution.md` design section, or `docs/ARCHITECTURE.md` for the diagram.

**Read:**

> "Here's how it fits together. It's a Semaphore-style scheme. Every member takes a
> secret, hashes it into a public leaf, and enrolls that leaf into a Merkle tree on
> chain. To approve a proposal, a member generates a zero-knowledge proof that their
> leaf is in that tree, plus a nullifier. The proof says 'I'm one of the enrolled
> members' without revealing which one, and the nullifier stops anyone from voting
> twice. Once the approvals reach the threshold M, an execute step releases the
> treasury."

> "Now the part that's specific to this bounty. The spec wants members to hold
> shielded accounts. So a member's secret isn't a throwaway. It's their real
> shielded-account key, the nsk, the same key that controls a real private account
> on LEZ, derived from the standard LEZ key tree. So being a member is tied to
> actually owning a shielded account. Control the key, you control the account."

> "I'll be straight about what that does and doesn't do. The member set is public.
> You can see there are three members, but the leaves are one-way hashes of those
> private keys, so they don't reveal anyone's account or identity. Which member
> approved stays hidden. And the binding is by derivation, not an in-circuit check
> that the account is live on chain at that instant. I asked the maintainer about
> that, and they confirmed binding by derivation is what they want here. The full
> account layout and the exact limits are in the solution doc."

> "The proof is a real RISC0 STARK, and it runs client-side, so the secret never
> leaves the member's machine. The interesting engineering was fitting this to the
> LEZ account model, which is really the core of the bounty. The existing public
> multisig needs fresh, zero-nonce accounts, which private accounts can't give you.
> So my runner reads the proposal's live nonce right before it proves, instead of
> assuming zero. And because only an account's owning program can move its balance,
> the treasury is owned by the authenticated-transfer builtin. Execute releases the
> funds by chaining a call into that program, rather than touching the balance
> directly."

---

## Segment 3 - full lifecycle, live, real proofs (~3 min)

> THE headline. Whole lifecycle end to end against a local LEZ sequencer at
> `RISC0_DEV_MODE=0`, so these are real STARKs. Satisfies the "real proof, dev mode
> zero" requirement.

**On screen:** terminal on Hetzner. Keep the `RISC0_DEV_MODE=0` you type visible.

```bash
cd /root/lez-v012
RISC0_DEV_MODE=0 scripts/lp0002-demo.sh
```

> "Let me run the whole thing end to end with one script. I'm typing dev mode zero
> myself so you can see it. That's the flag that forces real proving. RISC0 has a
> dev mode that fakes proofs to make development fast, and if I used it none of
> this would mean anything. So everything here is real."

> "First it deploys the multisig program to a local sequencer, enrolls three
> members, each with their real shielded-account key, and creates a proposal that
> needs two to approve."

(First approve starts, point at the prove line. Cut the ~2-min prove.)

> "Member zero approves. Real STARK, so a couple of minutes. It lands, and the
> count goes from zero to one. Member one approves with a different key, so a
> different nullifier. The count goes to two."

(Execute runs:)

> "Two of two is met, so execute releases the treasury to the recipient. The
> script checks the chain at the end: count two, treasury drained, recipient
> credited. Full private multisig lifecycle, real proofs, one command."

---

## Segment 4 - the Basecamp app: a real anonymous vote (~1.5 min)

**Opener (~10s):** run Basecamp's OWN plugin host against our module.

```bash
# (exact path is in basecamp/README.md)
<basecamp>/ui-host --name private_multisig_lp0002 --path basecamp/dist/private_multisig_lp0002/msig_plugin.so
# prints: ui-host: loaded plugin "private_multisig_lp0002" ... READY
```

> "Real quick, let me prove this is an actual Basecamp module, not a web page
> pretending to be one. I point Basecamp's own plugin loader at my module. It
> loads it and signals READY. Point it at a fake file and it just fails. So this
> is a real Basecamp plugin. Now I'll drive it."

**Cut to the GUI (VNC).** Wait for each result before the next step.

### 4.1 Derive my leaf - paste member 0's secret, click **Derive leaf**.

Paste: `72d2833e462b171ea3ad2676b9967703c9a8620dabf49883986f3f36377bdc65`

> "First I paste my member secret. This is my real shielded-account key, and I
> derive my leaf from it right here, locally. The key never leaves this machine.
> The leaf is the public value that went into the tree when I enrolled."

### 4.2 Cast vote - Section 3, click **Cast anonymous vote** (real proof, ~2-3 min).

> "Now I cast a vote. The app sends my key only to the local prover, runs the
> real membership proof, and submits it. Same real STARK as a second ago, just
> driven from the app. Give it a couple of minutes."

(When it lands:)

> "There it is. Recorded on chain, real transaction hash, count moved up. A
> non-member secret gets rejected before any proof even runs, and voting twice
> gets rejected as a double vote. So the secret field actually does the work. It's
> not decorative."

(Optional: cast member 1 = `eee2d6cf8978e0a0b67c03eb68f4c48c0d0f1cc3cf77ca9f671ad193a893720a` to reach 2 of 2 on camera.)

---

## Segment 5 - on-chain read-back + public testnet (~80 sec)

**On screen:** terminal on Hetzner, reading the live 2-of-3 run off testnet.

```bash
cd /root/lez-v012
# wallet-home-lp0002's config already points at https://testnet.lez.logos.co
NSSA_WALLET_HOME_DIR=/root/lez-v012/wallet-home-lp0002 \
  EXPECT_COUNT=2 EXPECT_TREASURY=0 EXPECT_RECIPIENT=20 \
  ./target/release/run_assert_state
# prints, live off testnet:
#   ASSERT proposal Hf84MVjY...: approval_count=2 (expect 2)
#   ASSERT treasury 12JLe9...: balance=0 (expect 0)
#   ASSERT recipient 4qCzn...: balance=20 (expect 20)
#   ALL ASSERTIONS PASSED
```

**Read:**

> "To prove I'm not just trusting the app, here's the two-of-three run read straight
> off the public testnet. The approval count is two, the treasury drained to zero,
> and the recipient got the money. That's the real threshold release, live on the
> public chain, not the app faking it."

**Optional, even stronger:** pull one approve straight off the chain by its hash.

```bash
NSSA_WALLET_HOME_DIR=/root/lez-v012/wallet-home-lp0002 \
  ./target/release/wallet chain-info transaction \
  --hash 09c9cf27dbfa715f263613e2db1c36c2ec5bb4bd31f0968dc6032699652f23ae
# -> Transaction is Some(PrivacyPreserving( ... public_account_ids: [Hf84MVjY...] ... ))
```

> "And here's the privacy part. I can pull any of these transactions straight off
> the chain by its hash, so it's clearly real. But the proposal only stores the
> member root, the proposal id, the count, and a list of scrambled nullifiers. No
> name, no member number, nothing that says who approved. The count is public, the
> voters are not. All the transaction hashes are in the solution doc."

---

## Segment 6 - wrap (~30 sec)

**On screen:** face cam.

> "Two honest notes. In the app, one person clicks all the votes, because I'm the
> only one running it. In a real setup each member would be a different person on
> their own machine, with their own key. And again, the member set is public. The
> anonymity is over which member approved, within that known set. I document that,
> and the other limits like the fixed tree size and the derivation-binding, in the
> write-up, instead of pretending they're not there."

> "Everything's linked in the description: the repo, the program ID, the
> transaction hashes, and the notes. Thanks for watching."

---

## Screenshots to grab while recording (for the PR body)

- `ui-host` "loaded plugin / READY" lines.
- GUI after a vote: "vote recorded on-chain", the real tx hash, count incremented (ideally 2/2).
- `lp0002-demo.sh` showing `RISC0_DEV_MODE=0`, a real prove, and the final asserts (count=2, treasury 0, recipient credited) in one frame.
- Testnet read-back: `run_assert_state` showing count=2, treasury 0, recipient 20, ALL ASSERTIONS PASSED, plus the `chain-info transaction` lookup of one approve hash.

## Recording tips

- A vote is ~2-3 min to prove. Record it, cut the dead time, but keep the
  `RISC0_DEV_MODE=0` line and the result (tx hash + count) in one continuous shot.
- Wait for each result/toast before the next click; the busy guard swallows clicks mid-prove.
- Audio matters more than framing. A confident take beats a silent screencast.
- One clean take per segment; retake only the segment that breaks.
