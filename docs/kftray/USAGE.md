## Kftray Desktop Usage

## Configuring Your First Port Forward

In a few simple steps, you can configure your first port forward:

1. **Launch the application**
2. **Open the configuration panel from the tray icon**
3. **Add a new configuration:**

   - Give it a unique alias and set if you want to set the alias as domain to your forward \*1
   - Indicate if the configuration is for a port forward for a service (common use) or a proxy (port forward to an endpoint via a Kubernetes cluster).
   - Specify the Kubernetes context
   - Define the namespace housing your service
   - Enter the service name
   - Choose TCP or UDP
   - Set the local and remote port numbers. For a pod label target, the remote port list shows each port name and number once, even when several replicas match the label.
   - Configure a custom local IP address (optional)

4. **Activate Your Configuration**: Use the row switch to start or stop one port forward. Use **Start All** and **Stop All** to operate on multiple configurations.

> Note: To use the alias feature with a local domain name, you must enable write permissions in the hosts file. This method is not secure. We are addressing this in the following issue: [https://github.com/hcavarsan/kftray/issues/171](https://github.com/hcavarsan/kftray/issues/171).
> Follow these steps to allow write access:
>
> For Windows:
>
> ```bash
> icacls "C:\Windows\System32\drivers\etc\hosts" /grant Everyone:(R,W)
> ```
>
> For MacOS and Linux:
>
> ```bash
> sudo chmod ugo+rw /etc/hosts
> ```

## Forwarding behavior

- Service and pod configurations use the selected pod's container port as `remote_port`. A Service's exposed port is not translated a second time.
- Proxy configurations use `remote_address` and `remote_port` for the destination reached from the cluster. They do not require a `service` field.
- Proxy startup waits for the relay's TCP listener instead of a fixed delay. Existing startup probes are preserved; an unready sidecar does not prevent a started relay from forwarding.
- Expose waits for its server and reverse WebSocket handshake before reporting success. Kubernetes Service routing can still take time to converge after Service creation.
- HTTP logs use the same configuration directory as the database: `KFTRAY_CONFIG`, then `$XDG_CONFIG_HOME/kftray`, then `~/.kftray`. Logs are stored in its `http_logs` subdirectory.
- HTTP logs decode response bodies sent with `Content-Encoding: gzip`, `br` or a stacked value such as `gzip, br`. Other encodings are shown as received. A decoded body is cut off at 10 MiB, and the log entry notes the cut.
- Bulk actions run independent configurations concurrently in bounded batches. A busy configuration cannot start another operation until its current operation finishes.
- With rows selected, the toolbar shows **Start Selected** and **Stop Selected**. These act only on selected rows that the current search and filters show. **Start All** starts the shown configurations. **Stop All** stops every running configuration, shown or not.
- During a bulk action, the cancel button discards queued operations. Operations already in progress finish normally. Failed configurations are reported without blocking the rest of the batch.
- Stop All also stops expose tunnels. Stopping a configuration cancels its recovery before releasing its listeners and Kubernetes resources.
- If you leave **Local Port** blank, kftray picks a free port when you save and stores it. An empty alias is set to `<workload type>-<protocol>-<local port>`. When you edit a running configuration, kftray stops it, saves it and restarts it on the stored port.

## App mode

kftray runs in one of two modes. To change the mode, open the footer menu (☰) and select **Settings > App Mode**:

- **Tray** (default): the window opens from the tray icon or the global shortcut and hides when it loses focus. When no system tray is available (for example, GNOME without the AppIndicator extension), kftray runs in Window mode until a tray appears.
- **Window**: kftray runs as a regular app. The tray icon goes away, the window opens in the center of the screen with a taskbar or Dock entry, and it stays open when it loses focus.

In both modes the `–` button in the header hides the window (Tray) or minimizes it (Window), and the `×` button quits kftray. Launching kftray again while it is running brings the existing window to the front. Drag the window edges to resize it. kftray keeps the size for the next start. **Settings > Window > Reset Position** moves the window back to its default position.

## Crash reports

On first start kftray asks whether it may send crash reports. Nothing is sent before you answer, and nothing is sent when you decline. The choice is stored, and you can change it later under **Settings > Crash Reports**.

A report has the error type, where in the code it happened, the app version, the operating system name and version, the CPU architecture and the device model. Panic messages are sent only when they are fixed strings in the code, so a panic that carries a cluster address or a resource name is reduced to a file and line. Reports never include cluster names, namespaces, service names, aliases, kubeconfig files or your host name.

Reports go to a GlitchTip server run by the kftray maintainer at `glitchtip.cavarsa.app`. The proxy in front of it drops the client IP before the report is stored. Setting the `DO_NOT_TRACK` environment variable turns reporting off whatever the setting says, and development builds never send reports.

## Export configurations to a JSON file

1. Open the main menu in the footer
2. Select the `Export Local File` option
3. Choose a file name and location to save the JSON file
4. The JSON file will contain all your current configurations

You can then import this JSON file at any time to restore your configurations.

Example Json configuration File:

```json
[
 {
   "alias": "service-tcp-8080",
   "context": "kind",
   "kubeconfig": "/Users/henrique/.kube/config.bkp",
   "local_port": 8080,
   "namespace": "argocd",
   "protocol": "tcp",
   "remote_port": 8080,
   "service": "argocd-server",
   "workload_type": "service"
 },
 {
   "alias": "pod-tcp-8083",
   "context": "kind",
   "kubeconfig": "/Users/henrique/.kube/config.bkp",
   "local_port": 8083,
   "namespace": "argocd",
   "protocol": "tcp",
   "remote_port": 8083,
   "target": "app.kubernetes.io/component=server",
   "workload_type": "pod"
 },
 {
   "alias": "proxy-udp-5353",
   "context": "kind",
   "kubeconfig": "/Users/henrique/.kube/config.bkp",
   "local_port": 5353,
   "namespace": "argocd",
   "protocol": "udp",
   "remote_address": "coredns.cluster.local.internal",
   "remote_port": 5353,
   "workload_type": "proxy"
 },
 {
   "alias": "proxy-tcp-6443",
   "context": "kind",
   "kubeconfig": "/Users/henrique/.kube/config.bkp",
   "local_port": 8777,
   "namespace": "argocd",
   "protocol": "tcp",
   "remote_address": "test.homelab.cluster.internal",
   "remote_port": 80,
   "workload_type": "proxy"
 }
  ]

```

## Sharing the configurations through Git

now, with the local json saved, you can share your configurations with your team members by committing the JSON file to a GitHub repository. This allows for easy collaboration and synchronization of KFtray configurations across your team.

To import and sync your GitHub configs in kftray:

1. Open the application's main menu
2. Select the button with GitHub icon in the footer menu
3. Enter the URL of your Git repository and one or more paths to the JSON config file(s) (use the `+` button to add additional paths if your configs are split across multiple files)
4. If your GitHub repository is private, you will need to enter the private token. Credentials are securely saved in the SO keyring (Keychain on macOS). Kftray does not store or save credentials in any local file; they are only stored in the local keyring.
5. Select the polling time for when Kftray will synchronize configurations and retrieve them from GitHub.

6. KFtray will now sync with the Git repository to automatically import any new configurations or changes committed to the JSON file.

This allows you to quickly deploy any port forward changes to all team members. And if someone on your team adds a new configuration, it will be automatically synced to everyone else's KFtray.
