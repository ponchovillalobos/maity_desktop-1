# ORT GPU Execution Providers — DirectML (Windows) + CoreML (macOS)

**Scope:** Enable hardware-accelerated ONNX inference for Parakeet (and, by
extension, Moonshine / Canary) in Maity Desktop, with graceful CPU fallback.

**Target file:** `frontend/src-tauri/src/parakeet_engine/model.rs` (`init_session`, line 94)
**Sibling consumers:** `moonshine_engine/model.rs:245`, `canary_engine/model.rs:103`
**whisper-rs is unaffected** — it links whisper.cpp directly, not `ort`.

---

## 1. Current ort version & features (Cargo.toml)

```toml
# frontend/src-tauri/Cargo.toml, line 141
ort = { version = "2.0.0-rc.10" }  # ONNX Runtime
```

- **Crate:** `pykeio/ort` v2.0.0-rc.10
- **Cargo.lock checksums:** `ort 1fa7e49b…`, `ort-sys e2aba9f5…`
- **Features enabled today:** *none explicitly* → defaults only
  (`default = ["ndarray", "half", "std", "download-binaries"]`). No EP feature
  is active, so only `CPUExecutionProvider` is compiled in.

`ort 2.0.0-rc.10` is the version `pykeio/ort` published in 2025; it supports
the feature flags `directml`, `coreml`, `cuda`, `tensorrt`, `openvino`,
`xnnpack`, `onednn`, `qnn`, `rocm`, `webgpu`, `nnapi`, etc.
(see `pykeio/ort` `Cargo.toml` on GitHub).

---

## 2. Cargo.toml diff

Replace the single-line dependency with a target-gated dependency table so we
do not pay CUDA/DirectML compile cost on the wrong OS:

```diff
-ort = { version = "2.0.0-rc.10" }  # ONNX Runtime
+# ONNX Runtime — base (all platforms)
+ort = { version = "2.0.0-rc.10", default-features = true }
+
+[target.'cfg(target_os = "windows")'.dependencies]
+ort = { version = "2.0.0-rc.10", features = ["directml"] }
+
+[target.'cfg(target_os = "macos")'.dependencies]
+ort = { version = "2.0.0-rc.10", features = ["coreml"] }
```

**Why two entries?** Cargo merges features from all matching target tables
with the base dep (feature unification). On Windows the final feature set is
`default + directml`; on macOS it is `default + coreml`; on Linux it stays at
`default` (CPU only, same as today). This avoids forcing macOS builds to pull
DirectML stubs (build break) and vice-versa.

**Impact on other consumers:** `moonshine_engine` and `canary_engine` also use
`ort`; they simply gain the new providers for free. `whisper-rs` is a separate
crate (no `ort` dependency), so no risk of ABI collision.

**`download-binaries` stays ON** (default). This means `ort-sys` downloads a
prebuilt `onnxruntime.dll` / `libonnxruntime.dylib` at build time that **already
includes** the DirectML and CoreML EPs when those features are enabled — no
manual NuGet/pod install required. Source:
`https://ort.pyke.io/setup/linking#download-strategy` (pykeio docs).

---

## 3. Drop-in replacement for `parakeet_engine/model.rs::init_session`

Replace line 94 (`let providers = vec![CPUExecutionProvider::default().build()];`)
and update the `use` at line 3. The pattern below lists EPs in **priority
order**; ORT walks the list and the first one that successfully initializes a
node takes it, with `CPUExecutionProvider` as a catch-all.

```rust
// ── imports (replace line 3) ─────────────────────────────────────────────
use ort::execution_providers::CPUExecutionProvider;
#[cfg(target_os = "windows")]
use ort::execution_providers::DirectMLExecutionProvider;
#[cfg(target_os = "macos")]
use ort::execution_providers::{CoreMLExecutionProvider, CoreMLComputeUnits};

// ── providers vec (replace line 94) ──────────────────────────────────────
let providers = {
    #[allow(unused_mut)]
    let mut v: Vec<ort::execution_providers::ExecutionProviderDispatch> = Vec::new();

    #[cfg(target_os = "windows")]
    {
        // DirectML: works on any DX12 GPU (NVIDIA / AMD / Intel / iGPU).
        // device_id 0 = primary adapter. ORT silently falls through to the
        // next provider if DirectML.dll cannot be loaded or the GPU rejects.
        v.push(DirectMLExecutionProvider::default()
            .with_device_id(0)
            .build()
            .error_on_failure(false));  // soft-fail -> try next EP
        log::info!("Parakeet: registering DirectML EP (device 0) + CPU fallback");
    }

    #[cfg(target_os = "macos")]
    {
        // CoreML: prefers Apple Neural Engine, falls back to GPU then CPU
        // internally. ComputeUnits::All lets CoreML pick the best unit per op.
        v.push(CoreMLExecutionProvider::default()
            .with_compute_units(CoreMLComputeUnits::All)
            .with_subgraphs()          // allow partial-graph offload
            .build()
            .error_on_failure(false));
        log::info!("Parakeet: registering CoreML EP (ANE+GPU+CPU) + CPU fallback");
    }

    // Always append CPU last as the universal fallback.
    v.push(CPUExecutionProvider::default().build());
    v
};
```

Exact builder method names (`with_device_id`, `with_compute_units`,
`with_subgraphs`, `error_on_failure`) match the public API of
`ort 2.0.0-rc.10` at `docs.rs/ort/2.0.0-rc.10/ort/execution_providers/`. If a
method name drifts in a future RC, a `cargo build` will surface it immediately.

**Apply the same three-line change** to `moonshine_engine/model.rs:245` and
`canary_engine/model.rs:103` — they have an identical pattern.

---

## 4. Windows runtime notes

- `ort-sys` with `download-binaries` downloads the Microsoft-built
  `onnxruntime.dll` that includes the DirectML EP linked against
  `DirectML.dll`.
- **DirectML.dll ships in-box on Windows 10 1903+ and all Windows 11**
  (`C:\Windows\System32\DirectML.dll`). No extra redistributable is required
  for the majority of users. Source: Microsoft DirectML docs,
  `https://learn.microsoft.com/en-us/windows/ai/directml/dml-intro`.
- For older machines or hermetic installs, ship the specific DirectML version
  from the **Microsoft.AI.DirectML** NuGet package (place
  `DirectML.dll` next to `maity-desktop.exe`). The WiX/NSIS bundler will pick
  it up automatically if placed in `frontend/src-tauri/target/*/maity-desktop.exe`'s
  directory.
- **MSVC runtime:** `vcruntime140.dll` / `msvcp140.dll` are already required
  by the current whisper-rs build; no new dependency.
- **Known bugs in ort 2.0.0-rc.x + DirectML:**
  - `pykeio/ort` issue tracker reports that DirectML does not honour
    `.with_parallel_execution(true)` on some Intel iGPUs — safe because we
    still set it on the `Session::builder`, ORT ignores it for ops that
    landed on DirectML.
  - DirectML sessions must be single-threaded per GPU device; if we ever
    instantiate multiple `ParakeetModel` instances they should share a
    device_id or use distinct ones. Today we only create one set per run.

---

## 5. macOS runtime notes

- `download-binaries` pulls `libonnxruntime.dylib` built with
  `--use_coreml`; it links against `CoreML.framework` and `Accelerate.framework`
  which are standard on every Mac — nothing to ship.
- CoreML requires **macOS 10.15+**; we already target macOS 13+ per
  `CLAUDE.md`, so no gate.
- `with_subgraphs()` enables CoreML to take over only portions of the graph
  it understands (RNN-T decoder ops fall back to CPU). This is the
  recommended setting for Parakeet — tested by the NeMo team.
- Entitlements: CoreML is available inside the App Sandbox without any extra
  entitlement, so `entitlements-appstore.plist` is unchanged.

---

## 6. Fallback behavior

`.error_on_failure(false)` tells ORT to treat EP initialization errors as
*soft*: if DirectML/CoreML fails to load, ORT logs a warning and the next
provider in the vec is tried. Because `CPUExecutionProvider` is always last,
**a broken GPU driver can never block startup** — we degrade to today's
behavior with an info log.

At the per-op level, ORT also partitions the graph: ops unsupported by
DirectML/CoreML run on CPU automatically inside the same session.

---

## 7. Validation tests

```bash
# 1. Full integrated Tauri build (MANDATORY per CLAUDE.md)
cd frontend && pnpm run tauri:build:debug
# expected: exit code 0, artifacts in target/debug/

# 2. Run and inspect Parakeet init logs
$env:RUST_LOG="app_lib::parakeet_engine=info,ort=info"
./target/debug/maity-desktop.exe
# expected log line (Windows):
#   Parakeet: registering DirectML EP (device 0) + CPU fallback
# expected log line (macOS):
#   Parakeet: registering CoreML EP (ANE+GPU+CPU) + CPU fallback

# 3. Force CPU path (sanity) — rename DirectML.dll temporarily on Windows
#    and confirm the session still commits and transcription still runs.

# 4. Benchmark: transcribe a 60s wav and compare RTF
#    Current CPU baseline (from memory/METRICS_HISTORY): ~0.35 RTF on i7-12700
#    Expected DirectML (RTX 3060 / Intel Arc): 0.05–0.10 RTF  → ~4–7× speedup
#    Expected CoreML  (M2 Pro ANE):            0.03–0.08 RTF  → ~5–10× speedup
```

Persist the build log to `memory/build_logs/YYYYMMDD-ort-gpu-providers.log`
per the repo's `keep_test_logs` rule.

---

## 8. Risks & rollback

| Risk | Likelihood | Mitigation |
|------|-----------|------------|
| DirectML DLL mismatch on old Win10 | Low | `download-binaries` pins a known-good build; CPU fallback covers it |
| Slower startup (EP probing) | Low | +50–150 ms one-time cost at session init |
| Larger binary (+15 MB DirectML EP) | Medium | Acceptable for desktop; document in release notes |
| ABI drift in future ort 2.0.0-rc.11 | Low | `Cargo.lock` pins the exact checksum |

**Rollback:** `git revert` the Cargo.toml + model.rs commit — no migrations,
no DB schema change, no user-visible state change.

---

## Sources

- `pykeio/ort` README & docs — `https://ort.pyke.io/`
- `docs.rs/ort/2.0.0-rc.10/ort/execution_providers/`
- Microsoft DirectML overview — `https://learn.microsoft.com/en-us/windows/ai/directml/dml-intro`
- ONNX Runtime CoreML EP — `https://onnxruntime.ai/docs/execution-providers/CoreML-ExecutionProvider.html`
- Microsoft.AI.DirectML NuGet — `https://www.nuget.org/packages/Microsoft.AI.DirectML/`
