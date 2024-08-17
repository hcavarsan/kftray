# KFtui Usage Guide

## Configuring Your First Port Forward

Follow these simple steps to configure your first port forward using KFtui:

### Step 1: Launch the KFtui App

1. Open your terminal.
2. Launch the KFtui app by typing:

   ```bash
   kftui
   ```

### Step 2: Create a Configuration File

Currently, KFtui does not support adding configurations directly from the TUI (Text User Interface). This feature is under development. For now, you can create a JSON file and import it using the `i` hotkey. Below is the format of the JSON file:

```json
{
  "service": "productpage",
  "namespace": "bookinfo",
  "local_port": 9080,
  "remote_port": 9080,
  "context": "kind-kind-rc-version",
  "workload_type": "service",
  "protocol": "tcp",
  "alias": "bookinfo",
  "domain_enabled": true
}
```

### Step 3: Import the Configuration File

1. Save your JSON configuration file.
2. Open KFtui and press the `i` hotkey to import the saved JSON file.
3. Alternatively, you can add the configuration in the Kftray Desktop app, and it will be available in the TUI as well.

### Step 4: Activate Your Configuration

1. With your configuration imported, navigate to the list of configurations.
2. Select the configurations you need to start by pressing the `space` key. You can select all configurations by pressing `Ctrl + A`.
3. Start the selected configurations by pressing the `f` hotkey.
4. To stop the configurations, follow the same steps but select them in the "Stopping Configs" window. You can navigate between windows using the left and right arrow keys.

> **Note:** To use the alias feature with a local domain name, you must enable write permissions in the hosts file. This method is not secure. We are addressing this in the following issue: [https://github.com/hcavarsan/kftray/issues/171](https://github.com/hcavarsan/kftray/issues/171).

### Enabling Write Access to Hosts File

#### For Windows:

```bash
icacls "C:\Windows\System32\drivers\etc\hosts" /grant Everyone:(R,W)
```

#### For MacOS and Linux:

```bash
sudo chmod ugo+rw /etc/hosts
```

## Exporting Configurations to a JSON File

You can export your current configurations to a JSON file for backup or sharing purposes.

### Steps to Export:

1. Open KFtui.
2. Press the `e` hotkey.
3. Choose a location to save the JSON file and press `Enter`.
4. Type the file name to save the JSON file.
5. The JSON file will contain all your current configurations.

You can import this JSON file at any time to restore your configurations.

### Example JSON Configuration File:

```json
[
  {
    "service": "argocd-server",
    "namespace": "argocd",
    "local_port": 8888,
    "remote_port": 8080,
    "context": "test-cluster",
    "workload_type": "service",
    "protocol": "tcp",
    "remote_address": "",
    "local_address": "127.0.0.1",
    "alias": "argocd",
    "domain_enabled": true
  }
]
```

## Commands

Press the `h` hotkey to display the help section, which includes the following commands:

- **Ctrl+C**: Quit
- **↑/↓**: Navigate
- **←/→**: Switch Table
- **f**: Start/Stop Port Forward
- **Space**: Select/Deselect
- **Ctrl+A**: Select/Deselect All
- **h**: Show Help
- **i**: Import
- **e**: Export
- **d**: Delete Selected
- **Tab**: Switch Focus (Menu/Table)
- **Enter**: Select Menu Item
- **c**: Clear Output
