# `flowsh` ⚡ *(v0.1.1)*

> **Interactive TUI Shell Script Editor with Live Line-by-Line Execution & Persistent Bash State**

[![License: MIT/Apache-2.0](https://img.shields.io/badge/License-MIT%2FApache--2.0-blue.svg)](#-license)
[![Language: Rust](https://img.shields.io/badge/Language-Rust%202021-orange.svg)](https://www.rust-lang.org/)
[![Version](https://img.shields.io/badge/version-0.1.1-green.svg)](CHANGELOG.md)
[![Build & Test](https://github.com/DK8197/flowsh/actions/workflows/ci.yml/badge.svg)](https://github.com/DK8197/flowsh/actions)

---

`flowsh` is a modern Terminal User Interface (TUI) editor built in **Rust** (`ratatui` + `crossterm` + `portable-pty`). It bridges the gap between static shell scripting and interactive REPL environments.

Instead of editing a script blindly in Vim or VS Code and repeatedly running it in a separate terminal window, `flowsh` allows you to execute individual lines or multiline blocks directly against a **live, persistent background Bash process** while editing.

---

## ✨ Key Features

- **🔄 Persistent Shell State:** Environment variables (`export`), working directory changes (`cd`), and custom shell functions persist seamlessly across line executions.
- **⚡ Compound Block Awareness:** Automatically detects and groups multiline shell constructs (`if`/`then`/`fi`, `for`/`do`/`done`, `while`, `case`) so they run as cohesive units instead of failing line-by-line.
- **📡 Async Live Streaming:** Asynchronous execution engine streams `stdout` and `stderr` in real time without blocking the UI thread.
- **🛑 Safe Process Cancellation:** Press `Ctrl+C` to send `SIGINT` signals to running child commands without killing your persistent Bash session.
- **🔍 Execution History Browser:** Quickly browse, search, and inspect outputs from past executed commands (`Ctrl+R` / `F6`).
- **🛡️ Clean History Isolation:** Your `flowsh` session history is cleanly isolated from your local system (`~/.bash_history`).
- **↩️ Editor Polish:** Full undo/redo patch engine (`Ctrl+Z` / `Ctrl+Y`), smooth viewport auto-scrolling, block closer auto-completion, and batch line execution (`Ctrl+E`).

---

## 📸 Architecture Overview

```text
  ┌─────────────────────────────────────────────────────────────┐
  │  flowsh TUI Editor                                          │
  │                                                             │
  │  1  export DB_HOST="localhost"                   [DONE]     │
  │  2  cd /tmp                                      [DONE]     │
  │  3  for i in {1..3}; do                      ──┐            │
  │  4    echo "Connecting to $DB_HOST ($i)"       │ Block      │
  │  5  done                                         ──┘ [RUN]  │
  └─────────────────────────────────────────────────────────────┘
  ┌─────────────────────────────────────────────────────────────┐
  │  Live Streamed Output Pane (stdout / stderr)                │
  │  Connecting to localhost (1)                                │
  │  Connecting to localhost (2)                                │
  │  Connecting to localhost (3)                                │
  └─────────────────────────────────────────────────────────────┘
🚀 Get Running in SecondsPrerequisitesRust Toolchain (1.75+ recommended): Install RustBash Shell installed in environment path (/bin/bash)Python 3 + pytest (optional, for running the live-PTY integration suite)Build & Run CommandsBash# Clone the repository
git clone [https://github.com/DK8197/flowsh.git](https://github.com/DK8197/flowsh.git)
cd flowsh

# Build release binary
cargo build --release

# Run flowsh on a script file
./target/release/flowsh path/to/script.sh

# Or open an empty buffer
./target/release/flowsh
⌨️ KeybindingsKeybindingActionCtrl+Enter / Ctrl+J / F5Execute the current line or compound block asynchronouslyCtrl+EBatch execute all non-blank lines/blocks above the cursorCtrl+CInterrupt running process (SIGINT) or stop Ctrl+E batch executionCtrl+R / F6Toggle execution history browserCtrl+SSave current fileCtrl+QQuit editor (gracefully closes background Bash session)Ctrl+OToggle focus between editor pane and output paneCtrl+Z / Ctrl+YUndo / Redo editsEnterInsert newline (auto-inserts matching done/fi for new blocks)Arrows / Home / EndCursor navigation (viewport auto-scrolls to follow)ℹ️ Terminal Protocol Compatibility Notes:Ctrl+Enter vs Ctrl+J: Most legacy terminals send 0x0A (Ctrl+J) for Ctrl+Enter. flowsh supports Kitty keyboard protocols (WezTerm, Kitty, Foot, iTerm2), fallbacks to Ctrl+J, and supports F5 universally across all terminals.Ctrl+R History: Avoids hijacking Ctrl+H (which resolves to Backspace on many terminal emulators).Ctrl+C Interrupts: Emulates real shell behavior—sends SIGINT to the foreground process while preserving your persistent session state.🧪 Testing Matrixflowsh utilizes a dual-layer testing architecture:Bash# 1. Internal Rust unit tests (editor buffer, cursor, block detection logic)
cargo test

# 2. Live PTY shell-behavior integration suite (pipes, redirects, $? propagation)
cargo build --release
python3 tests/shell_behavior_test.py
What the PTY Integration Suite Tests:Live streaming output (stdout & stderr separation)Working directory & environment variable persistence (cd, export)Exit status propagation ($?)Multiline block execution (for, while, if, case)Process cancellation (SIGINT resilience)History file isolation (HISTFILE=/dev/null)🏗️ Architecture & Engine DesignPlaintextsrc/
├── main.rs         Entry point: terminal setup, event loop, polls executor events
├── app/            AppState (single source of truth), App controller, event bus
├── editor/         Buffer, Cursor, Editor, undo/redo patch stack, block detection
├── executor/       Async background execution worker — owns persistent Bash PTY session
├── pty/            BashProcess spawner & PtyManager (low-level send/receive primitives)
├── ui/             Layout renderers, editor scrolling viewport, live output view
├── storage/        File read/write operations
├── shortcuts/      Raw key events -> High-level Intents
└── models/         Plain data models (Line, Output)
Deep Dive: Core MechanicsAsync Non-Blocking Execution: ExecutionEngine runs in a dedicated background worker thread. Commands are wrapped with marker headers ({ <cmd> ; } 2>'<err>'; printf '\n<marker>%d\n' "$?") and outputs are streamed asynchronously to the main thread via channels.Interrupt & Resynchronization: When Ctrl+C is pressed, 0x03 is sent to the PTY. To handle interactive Bash's behavior of abandoning remaining command markers upon SIGINT, flowsh dispatches an independent probe marker to resynchronize stream tracking safely.Exit Code Propagation ($?): The execution wrapper preserves the exact return code (__shdev_ec=$?; ( exit $__shdev_ec )) so subsequent line executions receive the true $? from prior steps.Clean Session History: Runs HISTFILE=/dev/null and HISTCONTROL=ignorespace so internal execution wrappers never contaminate your personal system history (~/.bash_history).⚠️ Scope & Known LimitationsKeyword-Based Block Detection: Block detection uses a lightweight stack-based keyword scanner rather than a full Bash parser. Keywords inside comments or strings may occasionally require manual selection.Flattened Block Commands: Multiline blocks are flattened into compound command lines for PTY dispatching.Horizontal Scrolling: Very wide code lines will wrap or truncate depending on pane dimensions.🤝 ContributingContributions are warmly welcomed! Please refer to CONTRIBUTING.md and CODE_OF_CONDUCT.md.If you are modifying low-level PTY primitives, consult CLAUDE.md and .claude/steering/gotchas.md for documented terminal behavior edge cases.📄 LicenseDual-licensed under either of the following licenses at your option:Apache License, Version 2.0 (LICENSE-APACHE)MIT License (LICENSE-MIT)
