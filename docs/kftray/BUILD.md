# Building `kftray` from Source

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


4. **Launch the Application in Development Mode:**

   ```bash
   pnpm tauri dev
   ```

5. **Build the Application for Production:**

   ```bash
   pnpm tauri build
   ```


<br>
