#!/bin/bash

set -e

INSTALL_DIR="$HOME/.local/bin"
PROFILE_FILES=("$HOME/.profile" "$HOME/.bashrc" "$HOME/.zshrc" "$HOME/.config/fish/config.fish")
TMP_DIR="/tmp"
TMP_FILE="$TMP_DIR/kftui"

# Function to print messages in color with timestamp
print_msg() {
  local color="$1"
  local msg="$2"
  local timestamp=$(date +"%Y-%m-%d %H:%M:%S")
  case "$color" in
    red) echo -e "\033[31m[$timestamp] $msg\033[0m" ;;
    green) echo -e "\033[32m[$timestamp] $msg\033[0m" ;;
    yellow) echo -e "\033[33m[$timestamp] $msg\033[0m" ;;
    blue) echo -e "\033[34m[$timestamp] $msg\033[0m" ;;
    *) echo "[$timestamp] $msg" ;;
  esac
}

# Function to determine the OS
detect_os() {
  case "$(uname -s)" in
    Darwin) echo "macos" ;;
    Linux)
      if grep -q Microsoft /proc/version; then
        echo "wsl"
      else
        echo "linux"
      fi
      ;;
    *) print_msg red "Unsupported OS"; exit 1 ;;
  esac
}

# Function to determine the architecture
detect_arch() {
  case "$(uname -m)" in
    x86_64) echo "amd64" ;;
    arm64|aarch64) echo "arm64" ;;
    i386|i686) echo "x86" ;;
    *) print_msg red "Unsupported architecture"; exit 1 ;;
  esac
}

# Function to check if a command exists
command_exists() {
  command -v "$1" >/dev/null 2>&1
}

# Ensure required tools are available
if ! command_exists curl && ! command_exists wget; then
  print_msg red "Error: Neither curl nor wget is installed. Please install one of them and try again."
  exit 1
fi

# Get the latest release tag from GitHub
LATEST_RELEASE=$(curl -s https://api.github.com/repos/hcavarsan/kftray/releases/latest | grep '"tag_name":' | sed -E 's/.*"([^"]+)".*/\1/')
if [ -z "$LATEST_RELEASE" ]; then
  print_msg red "Error: Unable to fetch the latest release tag from GitHub."
  exit 1
fi

BASE_URL="https://github.com/hcavarsan/kftray/releases/download/${LATEST_RELEASE}"

# Determine OS and architecture
OS=$(detect_os)
ARCH=$(detect_arch)

# Set the download URL based on OS and architecture
case "$OS" in
  macos) URL="${BASE_URL}/kftui_macos_universal" ;;
  linux|wsl)
    if [ "$ARCH" = "amd64" ]; then
      URL="${BASE_URL}/kftui_amd64"
    elif [ "$ARCH" = "arm64" ]; then
      URL="${BASE_URL}/kftui_arm64"
    else
      print_msg red "Error: Unsupported architecture for Linux/WSL."
      exit 1
    fi
    ;;
  *)
    print_msg red "Error: Unsupported OS."
    exit 1
    ;;
esac

# Function to download a file using curl or wget
download_file() {
  local url=$1
  local output=$2

  if command_exists curl; then
    print_msg blue "Downloading kftui using curl from $url"
    if ! curl -L -s "$url" -o "$output"; then
      print_msg red "Error: Failed to download file using curl."
      curl -L "$url" -o "$output"  # Run without -s to show error
      exit 1
    fi
  elif command_exists wget; then
    print_msg blue "Downloading kftui using wget from $url"
    if ! wget "$url" -O "$output"; then
      print_msg red "Error: Failed to download file using wget."
      exit 1
    fi
  else
    print_msg red "Error: Neither curl nor wget is available for downloading."
    exit 1
  fi
}

# Function to download and install kftui
install_kftui() {
  local url=$1
  local filename=$2

  print_msg blue "Starting download and installation process for kftui"

  # Change to /tmp directory
  cd "$TMP_DIR"

  download_file "$url" "$filename"
  chmod +x "$filename"
  mkdir -p "$INSTALL_DIR"
  print_msg blue "Installing kftui to $INSTALL_DIR"
  mv "$filename" "$INSTALL_DIR/" || { print_msg red "Error: Failed to move kftui to $INSTALL_DIR."; exit 1; }
  print_msg green "kftui installed successfully"

  # Add $INSTALL_DIR to PATH if it's not already there
  if ! echo "$PATH" | grep -q "$INSTALL_DIR"; then
    for profile in "${PROFILE_FILES[@]}"; do
      if [ -f "$profile" ]; then
        case "$profile" in
          *config.fish) echo "set -U fish_user_paths $INSTALL_DIR \$fish_user_paths" >> "$profile" ;;
          *) echo "export PATH=$INSTALL_DIR:\$PATH" >> "$profile" ;;
        esac
      fi
    done
    export PATH="$INSTALL_DIR:$PATH"
    print_msg yellow "Added $INSTALL_DIR to PATH. Please restart your terminal or run 'source ~/.profile' to update your PATH."
  fi

  if [ -x "$INSTALL_DIR/kftui" ]; then
    print_msg green "kftui installation completed successfully"
  else
    print_msg red "Error: kftui installation verification failed."
    exit 1
  fi
}

# Install kftui
install_kftui "$URL" "$TMP_FILE"
