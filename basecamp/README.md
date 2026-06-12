# LP-0002 — Private Multisig (anonymous M-of-N voting) Basecamp module

A native **ui_qml** Logos Basecamp plugin (the only plugin type Basecamp v0.1.2
actually loads) for the LP-0002 anonymous multisig. Modeled on the LP-0016
forum plugin (`/root/forum-protocol/basecamp/`).

**The Vote button drives a REAL anonymous on-chain vote.** It is not a static
placeholder: pressing it calls the sidecar, which spawns the proven
`run_approve_secret` runner — a Merkle-membership proof + a proposal-bound
nullifier, submitted on-chain. (Contrast competitor #87: a static do-nothing
`<pre>`, in a mini-app type that does not even load in v0.1.2.)

## Files

| File | Purpose |
|------|---------|
| `manifest.json` / `metadata.json` | ui_qml manifest — name `private_multisig_lp0002`, category `governance`, author `jeefxM`, IID `com.logos.component.IComponent`, view `qml/Main.qml`, per-arch `.so`. |
| `src/MsigPlugin.{h,cpp}` | Basecamp `IComponent` plugin: `createWidget()` returns a `QQuickWidget` bound to `MsigBackend`. `Q_INIT_RESOURCE(msig_qml)`. |
| `src/MsigBackend.{h,cpp}` | Backend `Q_INVOKABLE`s: `deriveLeaf(secretHex)` (local SHA256, secret stays on-machine), `getStatus()` (sidecar `/status`), `castVote(secretHex)` (sidecar `/approve`). Talks to the sidecar over HTTP (`QNetworkAccessManager`). |
| `src/main.cpp` | Standalone preview app (`MsigApp`) — same backend + QML, no Basecamp. |
| `qml/Main.qml` | UI: 1 Derive my leaf · 2 Proposal status · 3 Cast anonymous vote (busy/progress for the ~134s prove; result = tx hash + count; errors like "not an enrolled member" / "already voted"). |
| `CMakeLists.txt` | Mirrors the LP-0016 template; adds `Qt6::Network`. |
| `msig-sidecar.mjs` | Localhost-only HTTP server. `GET /status` → spawns `run_read_status`. `POST /approve {secret_hex}` → spawns `run_approve_secret`, parses `approve tx_hash:`, polls until the count advances, returns `{tx_hash, approval_count}`. |
| `run-basecamp-lp0002.sh` | Starts the sidecar; launches the GUI. `MODE=app` (default) execs the standalone `MsigApp`; `MODE=basecamp` installs the module into the extracted Basecamp tree and launches it via AppRun. |
| `dist/private_multisig_lp0002/` | Ready-to-install Basecamp module dir (matches the bundled `forum/` layout: `msig_plugin.so` + `manifest.json` + `metadata.json` + `qml/Main.qml` + `icons/`). |
| `build/libmsig_plugin.so`, `build/MsigApp` | Build outputs. |

Companion Rust bin (in `examples/program_deployment/src/bin/`):
`run_read_status.rs` — assertion-free read-only proposal status as one JSON line
(`run_assert_state` asserts the treasury/recipient PDAs, which don't exist before
`execute`, so it's unsafe for `/status`).

## Build (already done on the box)

```bash
# plugin + preview app (Qt6 + CMake)
cd /root/lez-v012/basecamp && cmake -B build && cmake --build build -j4
# read-status runner (run_approve_secret is already built)
cd /root/lez-v012 && cargo build --release -p program_deployment --bin run_read_status
```

## What is verified HEADLESSLY (no GUI needed)

1. **Plugin compiles + links** → `build/libmsig_plugin.so` (Qt6::Network linked).
2. **Plugin LOADS in Basecamp v0.1.2's own host** (the LP-0016 decisive test) —
   both the build output and the packaged `dist/` copy:
   ```bash
   R=/mnt/HC_Volume_105854327/basecamp-app/squashfs-root
   LD_LIBRARY_PATH="$R/usr/lib" XKB_CONFIG_ROOT="$R/usr/share/X11/xkb" QT_QPA_PLATFORM=offscreen \
     timeout 3 "$R/usr/bin/ui-host" --name private_multisig_lp0002 \
     --path /root/lez-v012/basecamp/dist/private_multisig_lp0002/msig_plugin.so --socket msig
   # prints READY (dlopen + cast to IComponent OK), then blocks on the socket (exit 124 = healthy).
   # A bogus .so prints no READY and exits 1.
   ```
3. **QML parses clean** (the thing ui-host READY does NOT exercise — it only
   dlopens + casts; the QML is parsed in `createWidget()`). Run the standalone
   app offscreen and confirm no `qrc:/qml/Main.qml:<line>` errors:
   ```bash
   R=/mnt/HC_Volume_105854327/basecamp-app/squashfs-root
   LD_LIBRARY_PATH="$R/usr/lib" QT_QPA_PLATFORM=offscreen timeout 5 \
     /root/lez-v012/basecamp/build/MsigApp 2>&1 | grep -iE "\.qml:|qml .*error"
   # (only connection-refused noise from the startup getStatus() is expected; no .qml: lines = clean)
   ```
4. **deriveLeaf is byte-exact**: C++ `QCryptographicHash` on `"/lp0002/leaf/\x00"||0xA7*32`
   == `bde7026d…2fa`, matching `msig_core::member_leaf` and the on-chain enrolled leaf 0.
5. **Sidecar + runner path end to end** (`/tmp/msig_sidecar_smoke.sh`, DEV_MODE=1):
   - `GET /status` → `{ready:true, …, approval_count:0, threshold:2}`
   - `POST /approve` member 0 (`a7..a7`) → `{success:true, tx_hash:…, approval_count:1}` (real on-chain vote, 0→1)
   - `POST /approve` non-member (`deadbeef..`) → `{success:false, error:"…not an enrolled member…"}`, count stays 1
   - `POST /approve` member 0 AGAIN → rejected (proposal-bound nullifier already spent — no double vote), count stays 1

## What needs the user's VNC load-test

Driving the actual Basecamp GUI (rendering the QML, clicking the Vote button in
the running app). Every backend call the GUI makes is already proven headlessly
above; the GUI step confirms the rendered widget + button wiring.

---

## EXACT VNC STEPS (user runs these to load the module and cast a vote)

### A. Pick a chain target (do this first, OUTSIDE the GUI)

**Option 1 — self-contained local demo (recommended for a clean recording):**
In a box terminal, boot a local sequencer and create the proposal, leaving it
running. Start a sequencer on a fixed port + a fresh wallet home (DEV_MODE=1 for
speed), then `run_deploy`, `run_enroll`, `run_create_proposal` against it (see
`scripts/lp0002-verify-secret.sh` steps 3–6a for the exact config/boot/precondition
commands — copy those into a standalone script that does NOT kill the sequencer at
the end). Note the wallet-home dir and the port; export them in step C.

**Option 2 — live LEZ testnet:** use the committed `wallet-home-lp0002`
(`sequencer_addr = https://testnet.lez.logos.co`) with `RISC0_DEV_MODE=0`. The
LP-0002 proposal must already exist on testnet (deploy → enroll →
create_proposal landed) for `/status` to be `ready`.

### B. Two GUI paths — pick one

The launcher (`run-basecamp-lp0002.sh`) takes a `MODE` env:

- **`MODE=app` (default, recommended):** execs the standalone `MsigApp` — the
  SAME `qml/Main.qml` + `MsigBackend` as the Basecamp plugin, in its own window.
  This is exactly the pattern LP-0016 uses (`run-forum-testnet.sh` execs the
  standalone `ForumApp`). Simplest, most reliable for a recording.
- **`MODE=basecamp`:** installs `dist/private_multisig_lp0002/` into the
  **extracted** Basecamp tree (`squashfs-root/usr/plugins/`) and launches
  Basecamp via its **`AppRun`** (the extracted tree), so the on-disk module is
  discovered. (Launching the raw `.AppImage` would NOT work: it mounts its own
  read-only squashfs and never sees an on-disk install. AppRun runs the extracted
  tree, which DOES read `usr/plugins/`.) The plugin's load into this host is
  already proven headlessly by the `ui-host … READY` test above.

### C. Launch the sidecar + GUI (over VNC, DISPLAY :1)

```bash
# In a VNC terminal, set the chain target chosen in step A, then run the launcher.
# It starts the localhost sidecar (prints /health), then launches the GUI.

# --- LOCAL demo (standalone app) ---
export NSSA_WALLET_HOME_DIR=/mnt/HC_Volume_105854327/lp0002-localnet/<your-wallet-home>
export RISC0_DEV_MODE=1            # match the local sequencer
export MSIG_BIN_DIR=/root/lez-v012/target/release
/root/lez-v012/basecamp/run-basecamp-lp0002.sh                 # MODE=app default

# --- the SAME, but inside real Basecamp ---
MODE=basecamp /root/lez-v012/basecamp/run-basecamp-lp0002.sh

# --- TESTNET (standalone app): NSSA_WALLET_HOME_DIR defaults to wallet-home-lp0002,
#     RISC0_DEV_MODE defaults to 0 — run with no overrides ---
/root/lez-v012/basecamp/run-basecamp-lp0002.sh
```

### D. In the GUI (window title "Private Multisig (LP-0002)", or the Basecamp module)

1. (`MODE=basecamp` only) open the module list / app drawer and select
   **private_multisig_lp0002** (category: governance).
2. **Section 2 — Proposal status:** auto-loads on open; press **Refresh status**
   if needed. You should see the proposal id, member root, and `approvals 0 / 2`.
3. **Section 1 — Derive my leaf:** paste a member secret (e.g. `a7a7…a7` =
   member 0, 64 hex chars) and press **Derive leaf**. The leaf hash appears
   (`bde7026d…` for member 0) — computed locally; the secret never leaves the
   widget for derivation.
4. **Section 3 — Cast anonymous vote:** with the same secret still in the field,
   press **Cast anonymous vote**. The button shows *"Proving & submitting…
   (~134s)"* with a busy/progress bar (on testnet/`RISC0_DEV_MODE=0`; ~instant on
   a DEV_MODE=1 local sequencer). On success the result panel shows the **tx
   hash** and **approval_count now 1 / 2**.
5. (Optional) Repeat with member 1's secret (`4242…42`) to reach the threshold
   (`approvals 2 / 2 · THRESHOLD MET`). A non-member secret (`deadbeef…`) shows
   the red error *"not an enrolled member"*; voting twice with the same member's
   secret shows a double-vote rejection (proposal-bound nullifier already spent —
   both confirmed in the headless smoke tests).

### Member secrets (demo fixture)

| Member | Secret (64 hex) | Leaf |
|--------|-----------------|------|
| 0 | `a7a7…a7` (0xA7 × 32) | `bde7026dec1d3386bc7c459c166bd959836b119554e570a4d55d2ed7719ec2fa` |
| 1 | `4242…42` (0x42 × 32) | `ce15b1d5…` |
| 2 | `5c5c…5c` (0x5C × 32) | `7f6ce1af…` |
| non-member | `deadbeef…` (rejected) | — |
