// LP-0002 msig sidecar: a localhost-only HTTP server that fronts the proven Rust
// runners for the Basecamp plugin (MsigBackend talks to it over HTTP, mirroring
// how the LP-0016 forum-sidecar fronts the SDK). It does NOT re-implement any
// proving or chain logic — it spawns the already-verified binaries:
//
//   GET  /status          -> spawn run_read_status, return its JSON line
//                            {ready, proposal_id, member_root, approval_count, threshold}
//   POST /approve {secret_hex}
//                         -> spawn run_approve_secret with APPROVER_SECRET_HEX=<secret_hex>,
//                            parse the `approve tx_hash:` line, then poll run_read_status
//                            until approval_count advances (a block lands), and return
//                            {success, tx_hash, approval_count}. A non-member / double vote
//                            exits non-zero -> {success:false, error:<runner stderr>}.
//
// Env (set by run-basecamp-lp0002.sh; the runners read the sequencer URL from the
// wallet config under NSSA_WALLET_HOME_DIR, NOT a direct env var):
//   MSIG_REPO              repo root (default /root/lez-v012)
//   MSIG_BIN_DIR           dir holding run_read_status / run_approve_secret
//                          (default $MSIG_REPO/target/release)
//   NSSA_WALLET_HOME_DIR   wallet home whose wallet_config.json -> sequencer_addr (REQUIRED)
//   RISC0_DEV_MODE         0 (real STARK, ~134s) or 1 (fast fake receipts); must match the seq
//   MSIG_SIDECAR_PORT      listen port (default 8799), 127.0.0.1 only
//   MSIG_APPROVE_TIMEOUT_MS prove+submit hard cap (default 600000 = 10 min)
//   MSIG_BLOCK_WAIT_MS     poll budget for the count to advance after submit (default 60000)

import { spawn } from "node:child_process";
import { createServer } from "node:http";
import { appendFileSync } from "node:fs";

const REPO = process.env.MSIG_REPO || "/root/lez-v012";
const BIN_DIR = process.env.MSIG_BIN_DIR || `${REPO}/target/release`;
const PORT = parseInt(process.env.MSIG_SIDECAR_PORT || "8799", 10);
const APPROVE_TIMEOUT_MS = parseInt(process.env.MSIG_APPROVE_TIMEOUT_MS || "600000", 10);
const BLOCK_WAIT_MS = parseInt(process.env.MSIG_BLOCK_WAIT_MS || "60000", 10);
const LOG = process.env.MSIG_SIDECAR_LOG || `${process.env.HOME || ""}/.vnc/msig-sidecar.log`;

function log(s) {
  const line = `[${new Date().toISOString()}] [pid ${process.pid}] ${s}`;
  try { appendFileSync(LOG, line + "\n"); } catch {}
  try { process.stderr.write(line + "\n"); } catch {}
}

// Spawn a runner; resolve with {code, stdout, stderr}. The runners inherit our
// env (NSSA_WALLET_HOME_DIR, RISC0_DEV_MODE, plus any extra passed in `env`).
function runBin(bin, env = {}, timeoutMs = 120000) {
  return new Promise((resolve) => {
    const child = spawn(`${BIN_DIR}/${bin}`, [], {
      cwd: REPO,
      env: { ...process.env, ...env },
    });
    let stdout = "";
    let stderr = "";
    let timer = setTimeout(() => {
      log(`[${bin}] TIMEOUT after ${timeoutMs}ms -> kill`);
      try { child.kill("SIGKILL"); } catch {}
    }, timeoutMs);
    child.stdout.on("data", (d) => { stdout += d.toString(); });
    child.stderr.on("data", (d) => { stderr += d.toString(); });
    child.on("error", (e) => {
      clearTimeout(timer);
      resolve({ code: -1, stdout, stderr: stderr + `\nspawn error: ${e.message}` });
    });
    child.on("close", (code) => {
      clearTimeout(timer);
      resolve({ code, stdout, stderr });
    });
  });
}

// Take the LAST JSON-looking line from a runner's stdout (matches how the Qt
// backend parses the last `{`-prefixed line).
function lastJsonLine(stdout) {
  const lines = stdout.split("\n").map((l) => l.trim()).filter(Boolean);
  for (let i = lines.length - 1; i >= 0; i--) {
    if (lines[i].startsWith("{")) {
      try { return JSON.parse(lines[i]); } catch {}
    }
  }
  return null;
}

// Read the current proposal status (assertion-free run_read_status).
async function readStatus() {
  const r = await runBin("run_read_status", {}, 60000);
  const obj = lastJsonLine(r.stdout);
  if (!obj) {
    throw new Error(
      `run_read_status produced no JSON (code ${r.code}). stderr: ${r.stderr.slice(-300)}`
    );
  }
  return obj;
}

function sendJson(res, status, obj) {
  const body = JSON.stringify(obj);
  res.writeHead(status, { "Content-Type": "application/json" });
  res.end(body);
}

const server = createServer(async (req, res) => {
  const url = new URL(req.url, `http://127.0.0.1:${PORT}`);

  // ── GET /status ────────────────────────────────────────────────────────
  if (req.method === "GET" && url.pathname === "/status") {
    try {
      const st = await readStatus();
      log(`GET /status -> ${JSON.stringify(st)}`);
      sendJson(res, 200, st);
    } catch (e) {
      log(`GET /status ERROR ${e.message}`);
      sendJson(res, 200, { ready: false, error: String(e.message) });
    }
    return;
  }

  // ── POST /approve {secret_hex} ─────────────────────────────────────────
  if (req.method === "POST" && url.pathname === "/approve") {
    let body = "";
    req.on("data", (c) => { body += c; if (body.length > 1e6) req.destroy(); });
    req.on("end", async () => {
      let secretHex;
      try { secretHex = (JSON.parse(body || "{}").secret_hex || "").trim(); }
      catch { return sendJson(res, 200, { success: false, error: "bad JSON body" }); }
      if (!/^(0x)?[0-9a-fA-F]{64}$/.test(secretHex)) {
        return sendJson(res, 200, { success: false, error: "secret_hex must be 64 hex chars (32 bytes)" });
      }
      const norm = secretHex.replace(/^0x/, "");

      // Record the pre-vote count so we can confirm the on-chain increment.
      let beforeCount = null;
      try { const st = await readStatus(); if (st.ready) beforeCount = st.approval_count; } catch {}

      log(`POST /approve secret=${norm.slice(0, 8)}… beforeCount=${beforeCount} (spawning run_approve_secret, may take ~134s)`);
      const r = await runBin("run_approve_secret", { APPROVER_SECRET_HEX: norm }, APPROVE_TIMEOUT_MS);

      // Parse `approve tx_hash: <hash>` from stdout.
      const m = r.stdout.match(/approve tx_hash:\s*(\S+)/);
      if (r.code !== 0 || !m) {
        // Surface the runner's own rejection text (non-member / double-vote / etc).
        const reason = (r.stderr.trim().split("\n").filter(Boolean).pop()
          || r.stdout.trim().split("\n").filter(Boolean).pop()
          || `run_approve_secret exited ${r.code}`).slice(0, 600);
        log(`POST /approve REJECT code=${r.code} reason=${reason}`);
        return sendJson(res, 200, { success: false, error: reason, exit_code: r.code });
      }
      const txHash = m[1];
      log(`POST /approve tx_hash=${txHash} — polling for count to advance (budget ${BLOCK_WAIT_MS}ms)`);

      // Poll run_read_status until the count advances past beforeCount (the tx
      // is only reflected once a block lands). Return the fresh count.
      let finalCount = beforeCount;
      const deadline = Date.now() + BLOCK_WAIT_MS;
      while (Date.now() < deadline) {
        await new Promise((rr) => setTimeout(rr, 3000));
        try {
          const st = await readStatus();
          if (st.ready) {
            finalCount = st.approval_count;
            if (beforeCount === null || st.approval_count > beforeCount) break;
          }
        } catch {}
      }
      log(`POST /approve DONE tx=${txHash} finalCount=${finalCount}`);
      sendJson(res, 200, { success: true, tx_hash: txHash, approval_count: finalCount });
    });
    return;
  }

  // ── GET /health ────────────────────────────────────────────────────────
  if (req.method === "GET" && url.pathname === "/health") {
    return sendJson(res, 200, { success: true, repo: REPO, binDir: BIN_DIR, devMode: process.env.RISC0_DEV_MODE ?? null });
  }

  sendJson(res, 404, { success: false, error: `no route ${req.method} ${url.pathname}` });
});

// Long /approve requests: do not let Node's default request timeout kill the
// socket mid-prove. 0 = unlimited.
server.requestTimeout = 0;
server.headersTimeout = 0;
server.keepAliveTimeout = 0;

server.listen(PORT, "127.0.0.1", () => {
  log(`msig sidecar listening on http://127.0.0.1:${PORT} (repo=${REPO}, binDir=${BIN_DIR}, devMode=${process.env.RISC0_DEV_MODE ?? "unset"}, walletHome=${process.env.NSSA_WALLET_HOME_DIR ?? "unset"})`);
});
