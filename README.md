# Slipstream

A local-first, high-performance desktop Markdown editor built with Tauri v2 and React.

## Features

- **Live Preview** — Split-pane editor with real-time Markdown rendering
- **Multi-Window Sync** — Content syncs across multiple windows in real time
- **Native Performance** — Rust-powered Markdown parsing via pulldown-cmark
- **File Operations** — Open, edit, and save Markdown files with native dialogs
- **Keyboard Shortcuts** — `Cmd+S` save, `Cmd+O` open, `Cmd+Shift+N` new window
- **macOS Native** — Transparent titlebar with overlay style

## Tech Stack

- **Frontend:** React 18, TypeScript, Tailwind CSS, Vite
- **Backend:** Rust, Tauri v2, pulldown-cmark

## Getting Started

### Prerequisites

- [Node.js](https://nodejs.org/) (v18+)
- [Rust](https://www.rust-lang.org/tools/install)
- [Tauri v2 prerequisites](https://v2.tauri.app/start/prerequisites/)

### Development

```bash
npm install
npm run tauri dev
```

### Build

```bash
npm run tauri build
```

## License

[Apache License 2.0](LICENSE)
