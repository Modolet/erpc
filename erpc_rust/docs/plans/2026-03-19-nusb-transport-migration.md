# NUSB Transport Migration Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Replace the current `rusb`-backed USB transport with a `nusb` implementation that preserves the existing async transport API while removing Tokio-only USB internals.

**Architecture:** Keep the public transport shape centered around `src/transport/rusb.rs` to avoid broad API churn, but swap the implementation to `nusb` bulk endpoints. Device discovery/open/claim stays on `MaybeFuture::wait()`, while bulk transfers use `nusb` endpoint futures plus a runtime-agnostic timeout wrapper.

**Tech Stack:** Rust, async-trait, futures, futures-timer, nusb

---

### Task 1: Add a regression test for interface and endpoint selection

**Files:**
- Create: `tests/rusb_transport_selection.rs`
- Modify: `src/transport/rusb.rs`

**Step 1: Write the failing test**

Add tests that require:
- selecting the first vendor-specific interface with both bulk IN and OUT endpoints
- rejecting a vendor-specific interface if either bulk endpoint is missing

**Step 2: Run test to verify it fails**

Run: `cargo test --test rusb_transport_selection`
Expected: FAIL because the new selection helpers do not exist yet.

**Step 3: Write minimal implementation**

Add a small selection helper and supporting types in `src/transport/rusb.rs` that can be reused by the `nusb` connection path.

**Step 4: Run test to verify it passes**

Run: `cargo test --test rusb_transport_selection`
Expected: PASS

**Step 5: Commit**

Run:

```bash
git add docs/plans/2026-03-19-nusb-transport-migration.md tests/rusb_transport_selection.rs src/transport/rusb.rs
git commit -m "test: 补充USB接口选择回归测试"
```

### Task 2: Replace the USB transport implementation

**Files:**
- Modify: `Cargo.toml`
- Modify: `src/client.rs`
- Modify: `src/transport/rusb.rs`

**Step 1: Write the failing test**

Use the selection regression tests as the red baseline, then compile the crate after dependency changes.

**Step 2: Run test to verify it fails**

Run: `cargo check`
Expected: FAIL while the old `rusb` implementation and new `nusb` types are inconsistent.

**Step 3: Write minimal implementation**

Replace blocking `rusb` calls with `nusb`:
- device discovery/open/claim via `.wait()`
- bulk IN/OUT via endpoint submission and completion futures
- runtime-agnostic timeout handling with `futures-timer`

**Step 4: Run test to verify it passes**

Run: `cargo check`
Expected: PASS

**Step 5: Commit**

Run:

```bash
git add Cargo.toml Cargo.lock src/client.rs src/transport/rusb.rs
git commit -m "feat: 使用nusb重写USB传输层"
```

### Task 3: Final verification

**Files:**
- Modify: `Cargo.toml`
- Modify: `Cargo.lock`
- Modify: `src/client.rs`
- Modify: `src/transport/rusb.rs`
- Create: `docs/plans/2026-03-19-nusb-transport-migration.md`
- Create: `tests/rusb_transport_selection.rs`

**Step 1: Run focused verification**

Run: `cargo test --test rusb_transport_selection`
Expected: PASS

**Step 2: Run full compile verification**

Run: `cargo check`
Expected: PASS

**Step 3: Commit final result**

Run:

```bash
git add Cargo.toml Cargo.lock src/client.rs src/transport/rusb.rs tests/rusb_transport_selection.rs docs/plans/2026-03-19-nusb-transport-migration.md
git commit -m "feat: 使用nusb替换rusb传输实现"
```
