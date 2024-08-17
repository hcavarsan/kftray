# Building `kftray` and `kftui` from Source


<br>

## `kftray` Desktop App

### Overview

`kftray` is a desktop application built using Tauri, which combines a Rust backend with a frontend built using React and Typescript

### Requirements

- **Node.js** and **pnpm** or **yarn** for building the frontend.
- **Rust** and **Cargo** for building the backend.
- **Tauri CLI** for managing the Tauri project.

For detailed prerequisites, please refer to the [Tauri Getting Started Guide](https://tauri.app/v1/guides/getting-started/prerequisites).

### Steps to Compile `kftray`

1. **Clone the Repository:**

   ```bash
   git clone https://github.com/hcavarsan/kftray.git
   ```

2. **Navigate to the Cloned Directory:**

   ```bash
   cd kftray
   ```

3. **Install Dependencies:**

   ```bash
   pnpm install
   ```

4. **Install Tauri CLI Globally (if not already installed):**

   ```bash
   pnpm add -g @tauri-apps/cli
   ```

5. **Launch the Application in Development Mode:**

   ```bash
   pnpm tauri dev
   ```

6. **Build the Application for Production:**

   ```bash
   pnpm tauri build
   ```


<br>

## `kftui` Terminal User Interface (TUI) App

### Overview

`kftui` is a terminal-based user interface application built using Rust and the Ratatui library.

### Requirements

- **Rust** and **Cargo** for building the application.
- **Git** for cloning the repository.

### Steps to Compile `kftui`

1. **Clone the Repository:**

   ```bash
   git clone https://github.com/hcavarsan/kftray.git
   ```

2. **Navigate to the `kftui` Directory:**

   ```bash
   cd kftray/kftui
   ```

3. **Build the Application:**

   ```bash
   cargo build --release
   ```

4. **Run the Application:**

   ```bash
   ./target/release/kftui
   # OR
   cargo run --bin kftui
   ```

<br>

---

### Additional Notes

- Ensure you have the latest stable version of Rust installed. You can install or update Rust using [rustup](https://rustup.rs/).
- The `cargo build --release` command will create an optimized binary in the `target/release` directory.
- If you encounter any issues during the build process, ensure all dependencies are up to date by running:

  ```bash
  cargo update
  ```



