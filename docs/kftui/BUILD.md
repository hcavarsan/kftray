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
   cd kftray
   ```

3. **Build the Application:**

   ```bash
   cargo build --release --bin kftui
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



