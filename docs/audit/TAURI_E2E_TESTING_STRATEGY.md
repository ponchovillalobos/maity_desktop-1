# Tauri 2.x End-to-End Testing Strategy — Maity Desktop

**Date:** 2026-04-08
**Author:** Assembly research pass
**Target:** Tauri 2.10.1 (verified in `frontend/src-tauri/Cargo.toml:162`), Next.js 14, Windows-first, macOS secondary
**Scope:** Automated E2E: launch app → click → verify backend state via invoke → measure latency → capture RAM/CPU

> **Research caveat.** Live WebSearch/WebFetch were unavailable during this pass, so the recommendations below are grounded in (a) the Tauri 2.x official testing docs as they stood through 2025, (b) the public state of `tauri-driver`, WebdriverIO, and Playwright ecosystems, and (c) direct inspection of this repository. Any URL cited below should be re-verified by a human before we commit test infra. Entries marked **[VERIFY]** are the highest priority for live confirmation.

---

## 1. Summary Table

| Tool | Click UI | Measure latency | RAM/CPU | Windows | Tauri 2.x | Setup effort | Verdict |
|---|---|---|---|---|---|---|---|
| **tauri-driver + WebdriverIO** | Yes | Yes (WDIO timers) | Via sidecar | Yes (Edge WebDriver) | Yes (stable in 2025) | 4-6 h | **RECOMMENDED for UI flows** |
| tauri-driver + Selenium (Rust/Python) | Yes | Yes | Via sidecar | Yes | Yes | 6-8 h | Works, worse DX than WDIO |
| Playwright official | Yes (browsers only) | Yes | N/A | N/A | **No** — cannot attach to WebView2/WKWebView | N/A | **Not viable** (see §3) |
| `@tauri-apps/playwright` community fork | Partial | Partial | Partial | Partial | Experimental **[VERIFY]** | 8-12 h | Too immature to bet on |
| Spectron-style | — | — | — | — | Dead (Electron-only, archived) | — | **Not viable** |
| PowerShell + pyautogui | Yes (pixel) | Weak | Yes (external) | Yes | N/A | 2-3 h | Smoke-test fallback only |
| Headless Tauri (`visible:false`) + WDIO | Yes | Yes | Yes | Yes (windowed, hidden) | Yes | +1 h on top of WDIO | **Combine with WDIO** |
| **Rust in-process command tests** | **No UI** | **Yes (nanos)** | Yes (sysinfo) | Yes | Yes | 2-3 h | **RECOMMENDED for backend** |
| cargo-nextest | N/A runner only | — | — | Yes | Yes | 30 min | **Add as runner** |

---

## 2. Top Recommendation — Two-Layer Strategy

Do **not** pick one tool. Maity's priorities (zero data loss, post-session speed) split naturally into two testing layers, and forcing everything through a single UI driver is slow, flaky, and blind to the parts that matter most.

### Layer A — Rust in-process integration tests (80% of coverage)

- Spin up the Tauri command handlers in `#[tokio::test]` functions using `tauri::test::mock_builder()` + `mock_app()` (Tauri 2.x `tauri::test` module, stable since 2.0). No window, no WebView2, no driver.
- Directly exercise `invoke_handler` commands (`start_recording`, `stop_recording`, `get_transcription_state`, whisper preload, etc.).
- Measure latency with `std::time::Instant`.
- Capture RSS + CPU with the `sysinfo` crate (already common in the repo's dep tree).
- Runner: **cargo-nextest** for parallelism and per-test timeouts.

**Why this is the biggest win:** It's the only layer where "post-session speed" and "zero data loss" can be asserted deterministically. No sleep/poll hacks. No driver flakiness. Runs on every PR in <60 s. Works headless in GitHub Actions with zero display setup.

### Layer B — tauri-driver + WebdriverIO (20% of coverage, smoke/golden-path)

- One or two E2E scenarios: "launch → login state → start recording → UI shows Recording → stop → final summary visible".
- Drives real WebView2 on Windows and WKWebView on macOS via `tauri-driver` (a Rust binary that proxies W3C WebDriver to the native webviews).
- Runs pinned to a **debug build** (artifact from `pnpm run tauri:build:debug` per CLAUDE.md).
- On Windows CI: `msedgedriver` must match the installed Edge WebView2 runtime version. **[VERIFY]** this is still the requirement in Tauri 2.x docs.

**Justification for WDIO over raw Selenium:** better selector ergonomics (`$('button=Grabar')`), built-in waitFor helpers, native JSONL reporter, first-class async, and matches the Tauri docs' canonical example. WDIO also lets you share TypeScript types with the frontend repo.

**Justification against Playwright:** Playwright only drives Chromium, Firefox, and WebKit processes that **it** launches. It cannot attach to the WebView2 that Tauri embeds, and the `@tauri-apps/playwright` experimental package has never reached a v1 and is not recommended for production CI as of my knowledge cutoff. **[VERIFY]** before discarding permanently.

---

## 3. Why Not Playwright (detail)

Playwright's architecture assumes it owns the browser process and talks to it via CDP (Chromium) or a custom WebKit protocol. Tauri embeds the OS webview (WebView2 / WKWebView / WebKitGTK) inside a native window it controls. There is no CDP endpoint exposed by default, and even if you launch WebView2 with `--remote-debugging-port=...`, the attach story is fragile and not officially supported by either Playwright or Tauri. Any solution that works today can break on the next WebView2 auto-update. **Do not build your test strategy on this.**

---

## 4. Concrete Setup — Layer A (Rust in-process)

### 4.1 Install

```bash
# From repo root
cargo install cargo-nextest --locked
```

Add to `frontend/src-tauri/Cargo.toml`:

```toml
[dev-dependencies]
tokio = { version = "1", features = ["macros", "rt-multi-thread", "time", "test-util"] }
sysinfo = "0.33"
serde_json = "1"
tempfile = "3"
# tauri already re-exports tauri::test under the default feature set in 2.x
```

### 4.2 File layout

```
frontend/src-tauri/tests/
  e2e_recording.rs         # Layer A integration tests
  support/
    mod.rs                 # mock_app builder + metrics helpers
    metrics.rs             # RAM/CPU/latency capture → JSONL
```

### 4.3 Sample test — record, stop, verify transcription contains keywords

```rust
// frontend/src-tauri/tests/e2e_recording.rs
mod support;
use support::{metrics::MetricsRecorder, spawn_test_app};
use std::time::{Duration, Instant};

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn record_stop_verify_summary() {
    let mut metrics = MetricsRecorder::new("record_stop_verify_summary");
    let app = spawn_test_app().await;

    // 1. Wait for Whisper preload (command exposed by engine.rs)
    metrics.mark("whisper_preload_start");
    let preload = app.invoke::<bool>("whisper_is_ready", ()).await.unwrap();
    assert!(preload, "whisper not ready");
    metrics.mark("whisper_preload_done");

    // 2. Fire the same command the "Grabar" button fires
    let t0 = Instant::now();
    app.invoke::<()>("start_recording", serde_json::json!({
        "mic_device": "default",
        "system_device": "default",
    })).await.unwrap();
    metrics.record_latency("click_to_recording_state", t0.elapsed());

    // 3. Feed a known WAV fixture into the pipeline (test-only command)
    app.invoke::<()>("test_inject_wav", serde_json::json!({
        "path": "tests/fixtures/hello_world_es.wav"
    })).await.unwrap();

    // 4. Stop and wait for final transcript
    let t1 = Instant::now();
    app.invoke::<()>("stop_recording", ()).await.unwrap();
    let transcript: String = app.invoke("await_final_transcript",
        serde_json::json!({"timeout_ms": 5000})).await.unwrap();
    metrics.record_latency("stop_to_final_transcript", t1.elapsed());

    assert!(transcript.to_lowercase().contains("hola"),
        "expected 'hola' in transcript, got: {transcript}");

    metrics.sample_process();           // RSS + CPU snapshot
    metrics.flush_jsonl("memory/build_logs/e2e_metrics.jsonl");
}
```

The commands `whisper_is_ready`, `test_inject_wav`, and `await_final_transcript` are **test-only** commands to add behind `#[cfg(any(test, feature = "test-harness"))]` in the Rust side. This is the standard Tauri pattern for deterministic E2E. `test_inject_wav` bypasses `cpal` capture and writes directly into the `AudioPipelineManager` ring buffer.

### 4.4 Metrics capture helper (sketch)

```rust
// frontend/src-tauri/tests/support/metrics.rs
use std::{fs::OpenOptions, io::Write, time::{Duration, Instant}};
use sysinfo::{Pid, System};

pub struct MetricsRecorder {
    name: String,
    started: Instant,
    events: Vec<serde_json::Value>,
    sys: System,
    pid: Pid,
}

impl MetricsRecorder {
    pub fn new(name: &str) -> Self { /* ... */ }
    pub fn mark(&mut self, label: &str) { /* push {label, ms_since_start} */ }
    pub fn record_latency(&mut self, label: &str, d: Duration) { /* push */ }
    pub fn sample_process(&mut self) {
        self.sys.refresh_process(self.pid);
        if let Some(p) = self.sys.process(self.pid) {
            self.events.push(serde_json::json!({
                "kind": "proc_sample",
                "rss_mb": p.memory() / 1024 / 1024,
                "cpu_pct": p.cpu_usage(),
            }));
        }
    }
    pub fn flush_jsonl(&self, path: &str) {
        let mut f = OpenOptions::new().create(true).append(true).open(path).unwrap();
        for ev in &self.events {
            let line = serde_json::json!({
                "test": self.name, "event": ev, "ts": chrono::Utc::now().to_rfc3339()
            });
            writeln!(f, "{line}").unwrap();
        }
    }
}
```

Output lands in `memory/build_logs/e2e_metrics.jsonl` — matches the `keep_test_logs` memory rule and is directly consumable by the Assembly portal (`scripts/portal.py`).

### 4.5 Run

```bash
cd frontend/src-tauri && cargo nextest run --test e2e_recording
```

---

## 5. Concrete Setup — Layer B (tauri-driver + WebdriverIO)

### 5.1 Install **[VERIFY exact versions before use]**

```bash
# Rust side — tauri-driver binary
cargo install tauri-driver --locked

# Node side
cd frontend
pnpm add -D @wdio/cli @wdio/local-runner @wdio/mocha-framework @wdio/spec-reporter webdriverio
pnpm wdio config    # pick: local, mocha, typescript, spec reporter
```

On Windows also install Microsoft Edge Driver matching your Edge/WebView2 version and put `msedgedriver.exe` on PATH. `tauri-driver` spawns it for you but cannot download it.

### 5.2 `wdio.conf.ts` critical bits

```ts
export const config: WebdriverIO.Config = {
  runner: 'local',
  specs: ['./e2e/specs/**/*.ts'],
  maxInstances: 1,   // WebView2 doesn't parallelize cleanly
  hostname: '127.0.0.1',
  port: 4444,
  capabilities: [{
    'tauri:options': {
      application: '../src-tauri/target/debug/maity-desktop.exe',
    },
    browserName: 'wry',        // tauri-driver internal name
  }],
  framework: 'mocha',
  services: [
    ['tauri', { tauriDriverPath: 'tauri-driver' }],
  ],
  mochaOpts: { ui: 'bdd', timeout: 120_000 },
};
```

### 5.3 Sample spec — click Grabar, verify recording, stop, verify summary

```ts
// frontend/e2e/specs/record.spec.ts
import { $, browser, expect } from '@wdio/globals';
import fs from 'node:fs';

describe('Maity recording golden path', () => {
  it('records and produces a transcript', async () => {
    const t0 = Date.now();
    await browser.waitUntil(async () => (await $('[data-testid=whisper-ready]')).isExisting(),
      { timeout: 60_000, timeoutMsg: 'whisper never preloaded' });
    const preloadMs = Date.now() - t0;

    const tClick = Date.now();
    await $('button=Grabar').click();
    await expect($('[data-testid=recording-indicator]')).toBeDisplayed();
    const clickToRecording = Date.now() - tClick;

    await browser.pause(8000);  // real mic capture; replace with fixture inject when possible

    const tStop = Date.now();
    await $('button=Detener').click();
    const summary = await $('[data-testid=final-summary]');
    await summary.waitForDisplayed({ timeout: 15_000 });
    const stopToSummary = Date.now() - tStop;

    const text = await summary.getText();
    expect(text.toLowerCase()).toContain('hola');

    fs.appendFileSync('../memory/build_logs/e2e_metrics.jsonl', JSON.stringify({
      test: 'record_golden_path',
      preload_ms: preloadMs,
      click_to_recording_ms: clickToRecording,
      stop_to_summary_ms: stopToSummary,
      ts: new Date().toISOString(),
    }) + '\n');
  });
});
```

Add `data-testid` attributes to the React components so selectors aren't text-dependent (text changes with i18n; role selectors survive).

### 5.4 Run

```bash
cd frontend && pnpm tauri:build:debug && pnpm wdio run wdio.conf.ts
```

---

## 6. Metrics Capture Pattern

Single unified JSONL stream at `memory/build_logs/e2e_metrics.jsonl`:

```json
{"ts":"2026-04-08T20:01:03Z","test":"record_golden_path","layer":"B","event":"latency","label":"click_to_recording_ms","value":184}
{"ts":"2026-04-08T20:01:11Z","test":"record_golden_path","layer":"B","event":"latency","label":"stop_to_summary_ms","value":612}
{"ts":"2026-04-08T20:01:11Z","test":"record_golden_path","layer":"B","event":"proc_sample","rss_mb":412,"cpu_pct":23.1}
```

Portal ingestion: add a `/api/metrics/e2e` tail endpoint to `scripts/portal.py` that reads this file and plots latency over commits. This feeds the Assembly continuous-improvement loop required by `CLAUDE.md`.

For Layer B, use a Node companion (`pidusage` npm package) to sample the Tauri child process every 500 ms while the spec runs, since WebdriverIO has no built-in RSS capture.

---

## 7. CI — GitHub Actions Windows Runner

Layer A runs cleanly on `windows-latest`:

```yaml
- uses: dtolnay/rust-toolchain@stable
- uses: taiki-e/install-action@v2
  with: { tool: cargo-nextest }
- name: Rust E2E
  working-directory: frontend/src-tauri
  run: cargo nextest run --test e2e_recording --profile ci
```

Layer B needs an Edge WebView2 + msedgedriver + a desktop session. `windows-latest` runners **do** ship WebView2 Evergreen and Edge, but `tauri-driver` needs a real desktop — the runner is headful by default, so it works without Xvfb (unlike Linux). **[VERIFY]** that `tauri-driver` still supports the GHA `windows-latest` image in 2026; historically it did through 2024-2025.

```yaml
- uses: pnpm/action-setup@v4
- uses: actions/setup-node@v4
- run: pnpm install
  working-directory: frontend
- run: pnpm tauri:build:debug
  working-directory: frontend
- run: cargo install tauri-driver --locked
- run: pnpm wdio run wdio.conf.ts
  working-directory: frontend
  timeout-minutes: 15
```

macOS runners: same idea with `safaridriver` wired through tauri-driver. macOS support in `tauri-driver` has historically lagged Windows — treat Layer B on macOS as best-effort, not blocking.

---

## 8. How competitors do it (knowledge-cutoff snapshot)

- **Obsidian** (Electron): Playwright + custom Electron launcher via `_electron.launch()`. Not applicable to Tauri.
- **Raycast** (native macOS, not Tauri): proprietary harness; public info minimal.
- **1Password 8** (Electron): Playwright + Electron; heavy use of contract tests at the IPC layer — same two-layer philosophy recommended here.
- **Linear desktop** (Electron wrapper around web): almost all tests run against the web app in Playwright; the desktop shell has a tiny smoke suite. This is exactly the model for Maity: put heavy coverage in Layer A and keep Layer B minimal.

The pattern across all four: **thin UI smoke layer + thick backend/contract layer**. That's what §2 prescribes.

---

## 9. Action items

1. Add `tauri::test` harness scaffolding in `frontend/src-tauri/tests/support/` (2 h).
2. Add test-only commands `whisper_is_ready`, `test_inject_wav`, `await_final_transcript` behind `#[cfg(feature = "test-harness")]` (2 h).
3. Write first Layer A spec `e2e_recording.rs` (1 h).
4. Install `cargo-nextest`, wire into CI (30 min).
5. **[VERIFY]** current `tauri-driver` Tauri 2.x status via `cargo search tauri-driver` + the GitHub repo before sinking time into Layer B.
6. If verified: install WDIO, write one smoke spec, wire CI (4-6 h).
7. Extend `scripts/portal.py` with `/api/metrics/e2e` tail reader (1 h).

**Total effort to a working two-layer pipeline: ~12 hours**, of which the first 6 hours (Layer A only) already deliver 80% of the value and unblock the Assembly portal's continuous-improvement loop.

---

## 10. References to re-verify before commit

- `https://tauri.app/develop/tests/` — Tauri 2 testing overview **[VERIFY]**
- `https://tauri.app/develop/tests/webdriver/` — tauri-driver guide **[VERIFY]**
- `https://tauri.app/develop/tests/mocking/` — mockIPC API **[VERIFY]**
- `https://github.com/tauri-apps/tauri-driver` — latest release + Tauri 2 issues **[VERIFY]**
- `https://webdriver.io/docs/api` — WDIO 9.x API **[VERIFY]**
- `https://nexte.st/` — cargo-nextest docs **[VERIFY]**
- `docs.rs/tauri/2.10.1/tauri/test/index.html` — `tauri::test` module (confirmed present in 2.x)
- `docs.rs/sysinfo/latest/sysinfo/` — RSS/CPU API

Because live web fetch was unavailable during this pass, a human must confirm that `tauri-driver` still publishes a Tauri-2-compatible release before Layer B is committed.
