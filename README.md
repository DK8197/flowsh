# Flowsh ⚡(V 0.1.1)

### Write. Run. Flow.

Flowsh is a **terminal-native shell development environment** that lets you write, test, and iterate on shell scripts without constantly switching between your editor and terminal.

Think of it as a small, focused workspace for shell scripting:

```text
Edit → Run → See what happened → Fix → Run again
```

And the best part?

Your commands run inside the **same persistent Bash session**.

So this:

```bash
cd /tmp
export ENV=development
```

is still in effect when you run:

```bash
pwd
echo "$ENV"
```

Just like a real interactive shell.

---

## 🖥️ Why Flowsh?

Working on shell scripts often looks like this:

```text
        ┌─────────┐
        │  Editor │
        └────┬────┘
             │
           Save
             │
             ▼
        ┌─────────┐
        │ Terminal│
        └────┬────┘
             │
            Run
             │
             ▼
          Output
             │
             ▼
        Back to editor
             │
             └─────── 🔁
```

Flowsh tries to make that loop much simpler:

```text
        ┌─────────┐
        │  Flowsh │
        │         │
        │  Edit   │
        │    ↓    │
        │  Run    │
        │    ↓    │
        │ Observe │
        │    ↓    │
        │  Fix    │
        └────┬────┘
             │
             └─────── 🔁
```

No context switching.

No saving just to test one command.

Just **write, run, and keep flowing.**

---

## 🚀 Quick Start

### Build from source

You'll need Rust installed if you're building Flowsh yourself.

```bash
git clone https://github.com/DK8197/flowsh.git
cd flowsh

cargo build --release
```

Then:

```bash
./target/release/flowsh script.sh
```

On Windows:

```powershell
.\target\release\flowsh.exe script.sh
```

> Pre-built binaries will be provided in future releases.

---

<img width="1912" height="1016" alt="image" src="https://github.com/user-attachments/assets/71b79764-e718-4b16-aeaf-9451b73d4d48" />


## ✨ What can Flowsh do?

### Run commands interactively

Write a command:

```bash
echo "Hello from Flowsh"
```

Press:

```text
Ctrl+Enter
```

and immediately see the result.

You can also use:

```text
Ctrl+J
F5
```

---

### 🔄 Persistent shell state

This is where Flowsh gets interesting.

Run:

```bash
export ENV=production
```

Then:

```bash
echo "$ENV"
```

Output:

```text
production
```

The shell session stays alive between executions.

The same applies to:

```bash
cd /some/directory
```

Followed by:

```bash
pwd
```

The directory change persists.

---

### 📤 stdout & stderr

Flowsh keeps standard output and standard error separate.

```bash
echo "Everything is fine"

echo "Something went wrong" >&2
```

You can see exactly what came from where.

---

### ❌ Exit codes

Commands report their exit status:

```text
✔ Exit: 0
```

or:

```text
✖ Exit: 1
```

This makes it easy to spot failures while developing a script.

---

## 🧪 Try it

Create a file called `test.sh`:

```bash
#!/bin/bash

echo "Starting Flowsh test..."

export FLOWSH_TEST="hello_from_flowsh"

echo "Environment:"
echo "$FLOWSH_TEST"

mkdir -p /tmp/flowsh-test

cd /tmp/flowsh-test

echo "apple" > file1.txt
echo "banana" > file2.txt
echo "cherry" > file3.txt

echo "Files:"
ls -l

echo "Searching:"
cat file1.txt file2.txt file3.txt | grep banana

echo "Current directory:"
pwd

echo "Done!"
```

Open it:

```bash
flowsh test.sh
```

Then start executing commands as you build and experiment.

---

## 🧩 How it works

Flowsh uses a persistent pseudo-terminal (PTY) to communicate with Bash.

```text
                    Flowsh
                       │
                       ▼
              ┌─────────────────┐
              │      Editor     │
              └────────┬────────┘
                       │
                 Current command
                       │
                       ▼
              ┌─────────────────┐
              │    Executor     │
              └────────┬────────┘
                       │
                       ▼
              ┌─────────────────┐
              │   PTY Manager   │
              └────────┬────────┘
                       │
                       ▼
              ┌─────────────────┐
              │ Persistent Bash │
              │     Session     │
              └────────┬────────┘
                       │
                 stdout/stderr
                       │
                       ▼
              ┌─────────────────┐
              │     Output      │
              └────────┬────────┘
                       │
                       ▼
                    Flowsh
```

The important part is that **Bash doesn't restart after every command**.

That is what allows things like:

```bash
cd /tmp
```

followed by:

```bash
pwd
```

to behave naturally.

---

## 🏗️ Project Structure

Flowsh is intentionally split into small modules:

```text
src/
├── app/          # Application state & controller
├── editor/       # Text editing & cursor management
├── executor/     # Command execution
├── models/       # Core data structures
├── pty/          # Persistent Bash / PTY handling
├── shortcuts/    # Keyboard shortcuts
├── storage/      # File operations
└── ui/           # Terminal UI & rendering
```

The goal is to keep the editor, execution engine, PTY handling, and UI independent from each other.

---

## 🛠️ Built With

Flowsh is written in **Rust**.

Main technologies:

- 🦀 Rust
- 🖥️ Ratatui
- ⌨️ Crossterm
- 🔌 PTY
- 🐚 Bash

The long-term goal is to distribute Flowsh as a small, standalone native binary.

End users shouldn't need to install Rust just to use Flowsh.

---

## 🗺️ Roadmap

Flowsh is still young. Here's where we're heading.

### v0.1 — Core MVP

- [x] Terminal editor
- [x] Persistent Bash session
- [x] Execute current line
- [x] stdout capture
- [x] stderr capture
- [x] Exit codes
- [x] Persistent environment
- [x] Persistent `cd`
- [x] Open / save scripts
- [x] Keyboard shortcuts

### v0.1.x — Make it feel great

- [ ] Editor scrolling
- [ ] Undo / redo
- [ ] Async command execution
- [ ] Live output streaming
- [ ] Cancel running commands
- [ ] Better multi-line command handling
- [ ] More shell compatibility tests

### v0.2 — Developer experience

- [ ] Syntax highlighting
- [ ] ShellCheck integration
- [ ] Command history
- [ ] Execution history
- [ ] Config file
- [ ] Configurable shell
- [ ] Better error diagnostics

### 🌱 Future ideas

Some things we're exploring:

- Bash / Zsh / Fish support
- SSH sessions
- Container execution
- Plugins
- AI-assisted shell development

Nothing here is set in stone.

The project will evolve based on what people actually find useful.

---

## 🤝 Contributing

Flowsh is open source and contributions are welcome!

You don't need to be a Rust expert to help.

You can contribute by:

- 🐛 Reporting bugs
- 💡 Suggesting features
- 🧪 Testing on different Linux distributions
- 📖 Improving documentation
- 🦀 Writing Rust
- 💬 Sharing how you use Flowsh

If something feels awkward, confusing, or just doesn't work — **open an issue.**

That's useful feedback.

---

## ⚠️ Early Days

Flowsh is an early-stage project.

It's functional, but things will change as the project grows.

For now, consider it experimental and avoid using it for critical production scripts until the project matures.

---

## 📜 License

Flowsh is licensed under the **Apache License 2.0**.

See [LICENSE](LICENSE) for details.

---

## ⭐ Follow the Flow

If you find Flowsh useful, consider giving the project a ⭐ on GitHub.

It helps the project get discovered and lets me know that people are interested.

```text
Write.
   ↓
Run.
   ↓
Observe.
   ↓
Improve.
   ↓
Flow.
```

**Flowsh — shell scripting without breaking your flow.** ⚡
