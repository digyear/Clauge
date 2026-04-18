<p align="center">
  <img src="src-tauri/icons/clauge-mark.svg" alt="Clauge" width="120" />
</p>

<h1 align="center">Clauge</h1>

<p align="center">
  A developer toolkit for managing Claude Code sessions, terminals, and workflows — all in one window.
</p>

<p align="center">
  <a href="https://github.com/ansxuman/Clauge/blob/main/LICENSE"><img src="https://img.shields.io/badge/license-Apache%202.0-7c5cf8?style=flat-square" alt="License"></a>
  <a href="https://github.com/ansxuman/Clauge/stargazers"><img src="https://img.shields.io/github/stars/ansxuman/Clauge?style=flat-square&color=f5a623" alt="Stars"></a>
  <a href="https://github.com/ansxuman/Clauge/issues"><img src="https://img.shields.io/github/issues/ansxuman/Clauge?style=flat-square&color=4f94d4" alt="Issues"></a>
  <a href="https://github.com/ansxuman/Clauge/releases/latest"><img src="https://img.shields.io/github/v/release/ansxuman/Clauge?style=flat-square&color=1dc880" alt="Release"></a>
</p>

<p align="center">
  <a href="https://clauge.ssh-i.in">Website</a> ·
  <a href="https://clauge.ssh-i.in/changelog.html">Changelog</a> ·
  <a href="https://github.com/ansxuman/Clauge/issues">Report Bug</a> ·
  <a href="https://buymeacoffee.com/ansxuman">Buy me a coffee</a>
</p>

---

## Features

### Sessions & Terminals

- **Parallel sessions** — Run multiple Claude Code sessions on the same project with git worktree isolation
- **6 purpose modes** — Brainstorming, Development, Code Review, PR Review, Debugging, Custom
- **Embedded terminal** — GPU-accelerated (WebGL) with customizable font size
- **Shell panel** — Per-session shell terminal alongside Claude (Cmd+L), drag-to-resize
- **Session resume** — Import existing Claude Code sessions via Custom purpose
- **Auto-detection** — Discovers existing sessions and notifies when creating new ones

### Git Integration

- **Branch indicator** — Current branch and ahead/behind count in the status bar
- **File changes** — View modified/added/deleted files with color-coded status
- **Inline diff viewer** — Click any file to see the diff with syntax highlighting
- **Selective staging** — Stage/unstage individual files with checkboxes
- **Quick actions** — Commit, push, pull, stash, pop stash from the app
- **Branch management** — View all branches, switch with one click
- **Commit history** — Browse recent commits
- **Per-session git identity** — Set different name/email per session

### Usage Dashboard

- **Cost breakdown** — Total cost, API calls, cache hit rate, session count
- **Daily activity chart** — Visual spending trends
- **Model analytics** — Cost and usage per model (Opus, Sonnet, Haiku)
- **Project breakdown** — Cost per project with session counts
- **Tool usage** — Which tools Claude uses most (Read, Edit, Bash, etc.)
- **Shell commands** — Which commands Claude runs (git, npm, cargo, etc.)
- **Live usage bars** — Session and weekly limits with configurable refresh interval
- **Session key config** — Connect to claude.ai for live tracking

### Plugin Manager

- **Installed plugins** — Enable/disable with toggle switches
- **Marketplace** — Browse available plugins with install counts
- **One-click install/uninstall** — Manage plugins without the terminal

### Context Manager

- **Reusable snippets** — Create context rules, coding guidelines, or instructions
- **Attach to sessions** — Select contexts during or after session creation
- **Mid-session injection** — Add/remove contexts while Claude is running
- **CLAUDE.md integration** — Contexts written to CLAUDE.md with safe markers, no conflicts with existing content

### App Experience

- **Notification sound** — Chime when Claude needs your input (repeats until focused)
- **Dock bounce** — macOS dock icon bounces on action-required prompts
- **Close to tray** — Window hides on close, reopen from Dock or tray
- **Auto-launch** — Starts on macOS login
- **Auto-update** — Downloads updates in background, restart to apply
- **Dark/Light themes** — 6 accent colors, customizable font size
- **Skip permissions** — Per-session `--dangerously-skip-permissions` toggle
- **File drag & drop** — Drag files onto the terminal to paste their path

## Keyboard Shortcuts

| Shortcut | Action |
|----------|--------|
| `Cmd+N` | New session |
| `Cmd+1-9` | Switch session |
| `Cmd+B` | Toggle sidebar |
| `Cmd+L` | Toggle shell panel |

## Download

<a href="https://github.com/ansxuman/Clauge/releases/latest"><strong>Download for macOS →</strong></a>

## Development

**Requires:** [Bun](https://bun.sh), [Rust](https://rustup.rs) 1.77+, [Tauri CLI](https://tauri.app) v2

```bash
git clone https://github.com/ansxuman/Clauge.git
cd Clauge
bun install
bun run tauri dev
```

## Tech Stack

| | |
|---|---|
| **Frontend** | SvelteKit, Svelte 5 |
| **Backend** | Rust, Tauri v2 |
| **Terminal** | xterm.js (WebGL), portable-pty |

## Support

<a href="https://www.buymeacoffee.com/ansxuman" target="_blank"><img src="https://cdn.buymeacoffee.com/buttons/v2/default-yellow.png" alt="Buy Me A Coffee" height="40"></a>

## License

[Apache License 2.0](LICENSE)
