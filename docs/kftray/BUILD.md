# Building `kftray` from Source

### Overview

`kftray` is a desktop application built using Tauri, which combines a Rust backend with a frontend built using React and TypeScript. The project uses [mise](https://mise.jdx.dev) as a unified task runner and development environment manager.

### Requirements

The project uses mise to manage all tools and dependencies. You only need to install:

1. **[mise](https://mise.jdx.dev)** - Development environment manager
   ```bash
   curl https://mise.run | sh
   ```

That's it! mise will handle installing and managing:
- Node.js
- pnpm
- Rust (nightly toolchain)
- Cargo tools (cargo-llvm-cov, cargo-nextest, cargo-insta)
- Tauri CLI
- All system dependencies

### Quick Start

1. **Clone the Repository:**
   ```bash
   git clone https://github.com/hcavarsan/kftray.git
   cd kftray
   ```

2. **Setup Development Environment:**
   ```bash
   mise install          # Install all tools defined in .mise.toml
   mise run setup        # Install system dependencies and project deps
   ```

3. **Start Development:**
   ```bash
   mise run dev          # Launch app in development mode (tauri dev)
   ```

4. **Build for Production:**
   ```bash
   mise run build        # Build production app (tauri build)
   ```

### Available mise Tasks

Run `mise tasks` to see all available tasks:

**Development:**
- `mise run dev` - Start Tauri development mode
- `mise run build` - Build production application
- `mise run build:ui` - Build only the frontend UI
- `mise run build:analyze` - Build with bundle analysis

**Code Quality:**
- `mise run format` - Format frontend and backend code
- `mise run lint` - Lint with auto-fix enabled
- `mise run test:back` - Run Rust backend tests
- `mise run test:server` - Run Docker proxy tests
- `mise run check` - TypeScript type checking

**Pre-commit:**
- `mise run precommit` - Run format, lint, and tests
- `mise run precommit:hook` - Git pre-commit hook (auto-staged)

**Utilities:**
- `mise run setup` - Setup development environment
- `mise run generate-icons` - Generate application icons
- `mise run knip` - Detect unused exports

### System Dependencies

The `mise run setup` command automatically detects your OS and installs required dependencies:

**Linux** (Ubuntu/Debian/Fedora/Arch/openSUSE):
- webkit2gtk-4.1-dev
- build-essential
- libssl-dev
- libayatana-appindicator3-dev
- librsvg2-dev
- And more...

**macOS**:
- Xcode Command Line Tools
- Homebrew (if not installed)

**Windows**:
- Microsoft C++ Build Tools
- WebView2 Runtime

For detailed prerequisites, see [Tauri Prerequisites](https://v2.tauri.app/start/prerequisites/).

### Manual Installation (Without mise)

If you prefer not to use mise:

1. Install prerequisites manually (see Tauri docs)
2. Install pnpm: `npm install -g pnpm`
3. Install dependencies: `pnpm install`
4. Run dev mode: `pnpm tauri dev`
5. Build production: `pnpm tauri build`

<br>
