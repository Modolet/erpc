# RUSB Runtime-Agnostic Rework Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Replace the current `nusb`-based USB transport with a `rusb` implementation that remains runtime-agnostic and avoids the macOS dependency conflict introduced by `nusb`.

**Architecture:** Keep the public `RusbTransport` API and the USB interface-selection helpers intact. Move all blocking `rusb` operations onto a dedicated worker thread that owns the device handle and internal read buffer; the async transport methods communicate with that worker through channels and wait on runtime-agnostic futures with explicit timeouts.

**Tech Stack:** Rust, rusb, async-trait, futures, futures-timer, std::thread, std::sync::mpsc

---

### Task 1: Lock selection behavior with tests

**Files:**
- Test: `tests/rusb_transport_selection.rs`
- Modify: `src/transport/rusb.rs`

**Step 1: Write the failing test**

Add a test asserting that non-vendor interfaces are ignored while a later vendor-specific interface with both bulk endpoints is selected.

**Step 2: Run test to verify it fails**

Run: `cargo test --test rusb_transport_selection`
Expected: FAIL because the new case is not yet covered.

**Step 3: Write minimal implementation**

Update the selection helper only if needed to satisfy the new test while keeping the current public helper API stable.

**Step 4: Run test to verify it passes**

Run: `cargo test --test rusb_transport_selection`
Expected: PASS

**Step 5: Commit**

```bash
git add tests/rusb_transport_selection.rs src/transport/rusb.rs
git commit -m "test: 补充USB接口选择场景"
```

### Task 2: Replace nusb with rusb worker-thread transport

**Files:**
- Modify: `Cargo.toml`
- Modify: `src/transport/rusb.rs`
- Modify: `docs/plans/2026-03-19-rusb-runtime-agnostic-rework.md`

**Step 1: Write the failing test**

Use the existing USB selection test suite as the red baseline, then remove `nusb` from the manifest so compilation fails until the transport is rewritten.

**Step 2: Run test to verify it fails**

Run: `cargo test --test rusb_transport_selection`
Expected: FAIL due to unresolved `nusb` imports and transport types.

**Step 3: Write minimal implementation**

Rebuild `RusbTransport` around:
- synchronous `rusb` device discovery and interface claim in `connect`
- a dedicated worker thread owning the `DeviceHandle`
- command/response channels for send, receive, and close
- runtime-agnostic waiting with `futures::channel::oneshot` plus `futures-timer`

**Step 4: Run test to verify it passes**

Run: `cargo test --test rusb_transport_selection`
Expected: PASS

**Step 5: Commit**

```bash
git add Cargo.toml Cargo.lock src/transport/rusb.rs docs/plans/2026-03-19-rusb-runtime-agnostic-rework.md
git commit -m "fix: 使用rusb工作线程重写USB传输"
```

### Task 3: Full verification

**Files:**
- Modify: `Cargo.toml`
- Modify: `Cargo.lock`
- Modify: `src/transport/rusb.rs`
- Create: `docs/plans/2026-03-19-rusb-runtime-agnostic-rework.md`

**Step 1: Run focused tests**

Run: `cargo test --test rusb_transport_selection`
Expected: PASS

**Step 2: Run compile verification**

Run: `cargo check`
Expected: PASS

**Step 3: Run full test suite**

Run: `cargo test`
Expected: PASS

**Step 4: Commit**

```bash
git add Cargo.toml Cargo.lock src/transport/rusb.rs tests/rusb_transport_selection.rs docs/plans/2026-03-19-rusb-runtime-agnostic-rework.md
git commit -m "fix: 消除USB传输的nusb依赖冲突"
```
