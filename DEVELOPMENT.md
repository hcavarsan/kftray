# Development Guide

This guide covers the development workflow for kftray using mise as the unified task runner.

## Table of Contents

- [Prerequisites](#prerequisites)
- [Quick Start](#quick-start)
- [Development Workflow](#development-workflow)
- [Available Tasks](#available-tasks)
- [Project Structure](#project-structure)
- [Git Workflow](#git-workflow)
- [Testing](#testing)
- [Troubleshooting](#troubleshooting)

## Prerequisites

### Required: mise

The project uses [mise](https://mise.jdx.dev) to manage all development tools and tasks. You only need to install mise:

```bash
curl https://mise.run | sh
```

After installation, restart your terminal or run:

```bash
source ~/.bashrc  # or ~/.zshrc
```

That's it! mise will handle installing:

- Node.js 24
- pnpm (latest)
- Rust nightly (with required components)
- Cargo tools (cargo-llvm-cov, cargo-nextest, cargo-insta, tauri-cli)
- System dependencies (via `mise run setup`)

## Quick Start

```bash
# Clone the repository
git clone https://github.com/hcavarsan/kftray.git
cd kftray

# Install tools defined in .mise.toml
mise install

# Setup system dependencies and install project dependencies
mise run setup

# Start development
mise run dev
```

## Development Workflow

### 1. First Time Setup

```bash
# Install mise-managed tools (Node, Rust, pnpm, etc.)
mise install

# Install system dependencies (webkit, build tools, etc.)
# This detects your OS and installs the right packages
mise run setup
```

### 2. Daily Development

```bash
# Start development mode (hot reload enabled)
mise run dev

# In another terminal, run tests
mise run test:back

# Format and lint before committing
mise run format
mise run lint
```

### 3. Building

```bash
# Build production app
mise run build

# Build only the UI
mise run build:ui

# Build with bundle analysis
mise run build:analyze
```

## Available Tasks

Run `mise tasks` to see all available tasks. Here are the most commonly used:

### Development

| Task | Description |
|------|-------------|
| `mise run dev` | Start Tauri development mode (hot reload) |
| `mise run build` | Build production application |
| `mise run build:ui` | Build only the frontend UI |
| `mise run build:analyze` | Build with bundle size analysis |

### Code Quality

| Task | Description |
|------|-------------|
| `mise run format` | Format frontend (Prettier) and backend (rustfmt) |
| `mise run format:front` | Format only frontend code |
| `mise run format:back` | Format only backend code |
| `mise run lint` | Lint with auto-fix (ESLint + Clippy) |
| `mise run lint:front` | Lint frontend with auto-fix |
| `mise run lint:back` | Lint backend with auto-fix |
| `mise run lint:back:check` | Lint backend without auto-fix (CI mode) |
| `mise run check` | TypeScript type checking |

### Testing

| Task | Description |
|------|-------------|
| `mise run test:back` | Run Rust backend tests with coverage |
| `mise run test:server` | Run Docker proxy tests |

### Pre-commit

| Task | Description |
|------|-------------|
| `mise run precommit` | Run format, lint, and tests |
| `mise run precommit:hook` | Git hook version (format + lint + stage changes) |

### Utilities

| Task | Description |
|------|-------------|
| `mise run setup` | Setup development environment |
| `mise run generate-icons` | Generate application icons |
| `mise run knip` | Detect unused exports in frontend |

### Version Management

| Task | Description |
|------|-------------|
| `mise run bump:patch` | Bump patch version (0.0.x) |
| `mise run bump:minor` | Bump minor version (0.x.0) |
| `mise run bump:major` | Bump major version (x.0.0) |
| `mise run release:patch` | Bump patch and create release commit |
| `mise run release:minor` | Bump minor and create release commit |

## Project Structure

```text
kftray/
├── .mise.toml              # mise configuration (tools + tasks)
├── frontend/               # React + TypeScript UI
│   ├── src/
│   └── package.json
├── crates/                 # Rust workspace
│   ├── kftray-tauri/      # Desktop app (Tauri)
│   ├── kftui/             # Terminal UI
│   ├── kftray-server/     # Proxy relay server
│   ├── kftray-portforward/# Port forwarding logic
│   └── ...
├── hacks/                  # Utility scripts
│   ├── setup.sh           # OS-specific setup script
│   └── ...
└── docs/                   # Documentation
```

## Git Workflow

### Pre-commit Hook

The repository has an automatic pre-commit hook that:

1. Formats all code (Prettier + rustfmt)
2. Lints with auto-fix (ESLint + Clippy)
3. Stages the fixed files automatically

When you run `git commit`, the hook runs automatically. Your code will be formatted and linted before the commit is created.

To bypass the hook (not recommended):

```bash
git commit --no-verify
```

### Creating Pull Requests

1. Fork and clone the repository
2. Create a feature branch: `git checkout -b feature/my-feature`
3. Make your changes
4. Commit (pre-commit hook runs automatically)
5. Push and create a pull request

The CI will run:

- Format check
- Lint check (no auto-fix in CI)
- Backend tests with coverage
- Frontend bundle analysis

## Testing

### Backend Tests

```bash
# Run all backend tests with coverage
mise run test:back

# Run specific test
cargo test test_name
```

### Frontend Type Checking

```bash
# Check TypeScript types
mise run check
```

### Docker Proxy Tests

```bash
# Test TCP/UDP proxy functionality
mise run test:server
```

## Troubleshooting

### mise not found

After installing mise, restart your terminal or run:

```bash
source ~/.bashrc  # or ~/.zshrc
```

### System dependencies missing

Run the setup script again:

```bash
mise run setup
```

This will detect your OS and install missing dependencies.

### Tool version mismatch

Ensure you have the latest tools:

```bash
mise install  # Reinstall all tools
```

### Port already in use

The default development port is used by Tauri. Kill any process using the port:

```bash
# macOS/Linux
lsof -ti:PORT | xargs kill -9

# Windows
netstat -ano | findstr :PORT
taskkill /PID <PID> /F
```

### Pre-commit hook failing

The hook runs format and lint automatically. If it fails:

1. Check the error message
2. Fix the issue manually
3. Try committing again

Or run the checks manually:

```bash
mise run format
mise run lint
```

### Building fails on Windows

Ensure you have:

1. Microsoft C++ Build Tools installed
2. WebView2 Runtime installed
3. NASM installed (for some dependencies)

Run `mise run setup` to see detailed instructions.

### Node/pnpm/Rust version issues

mise manages all tool versions. If you're having version issues:

```bash
# Check installed versions
mise ls

# Reinstall tools
mise install --force
```

## Additional Resources

- [Tauri Documentation](https://tauri.app)
- [mise Documentation](https://mise.jdx.dev)
- [Contributing Guide](CONTRIBUTING.md)
- [Build Instructions](docs/kftray/BUILD.md)

## Getting Help

- Check the [Issues](https://github.com/hcavarsan/kftray/issues) page
- Join our [Slack community](https://join.slack.com/t/kftray/shared_invite/zt-2q6lwn15f-Y8Mi_4NlenH9TuEDMjxPUA)
- Read the [FAQ](https://kftray.app/faq)
