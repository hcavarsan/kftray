## Kftray Desktop Usage

## Configuring Your First Port Forward

In a few simple steps, you can configure your first port forward:

1. **Launch the application**
2. **The main window opens on its own. Later, open it from the tray icon**
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

> Note: Domain aliases are written to the system hosts file, and a custom local address other than `127.0.0.1` is added to the loopback interface. Both need administrator access. When a start would need it and `kftray-helper` is not running, kftray asks first:
>
> - **Install helper**: one administrator prompt installs `kftray-helper`; later starts need none. The helper can also be installed or removed from the footer menu.
> - **Continue without**: the start proceeds and the system asks for administrator access for each change (`pkexec` on Linux, an admin prompt on macOS, UAC on Windows). Tick **Don't ask again** to always take this path; kftray still stops asking once the helper is installed.
> - **Cancel**: nothing is started.
>
> A start that fails because administrator access was refused shows the same dialog with the error, and **Retry without** repeats the start.

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

- **Tray** (default): the window opens when kftray starts, from the tray icon or from the global shortcut, and hides when it loses focus. When no system tray is available (for example, GNOME without the AppIndicator extension), kftray runs in Window mode until a tray appears.
- **Window**: kftray runs as a regular app. The tray icon goes away, the window opens in the center of the screen with a taskbar or Dock entry, and it stays open when it loses focus.

In both modes the `–` button in the header hides the window (Tray) or minimizes it (Window), and the `×` button quits kftray. Launching kftray again while it is running brings the existing window to the front. Drag the window edges to resize it. kftray keeps the size for the next start. **Settings > Window > Reset Position** moves the window back to its default position.

## Crash reports

On first start kftray asks whether it may send crash reports and performance data. Crash reports start on and performance data starts off. Nothing is sent before you answer. **Save** and closing the dialog keep what the switches show, and **No thanks** turns both off. The choices are stored, and you can change them later under **Settings > Crash Reports** and **Settings > Performance Data**.

A report has the error type, where in the code it happened, the app version, the build target (for example `x86_64-unknown-linux-gnu`), the operating system name and version, the CPU architecture, the device model, how many seconds the app had been running, the name of the thread that failed and a random id for the current run. The run id changes every time the app starts and is not stored anywhere, so it links the reports of one run without identifying you or your computer. Panic messages are sent only when they are fixed strings in the code, so a panic that carries a cluster address or a resource name is reduced to a file and line. A report also lists what happened before it (for example `portforward.start ok`, `window.shown`, `update.check`): only fixed names and whether an operation succeeded, never which configuration it was. A port forward, git import or MCP server start that fails is also reported on its own, as the operation name and a fixed reason from a short list (for example `timeout`, `target_not_found`, `kubeconfig`, `port_in_use`); the error message itself is never sent. Errors in the window are reported by the window itself, under the same switch and the same run id: only the error type, the code locations and the names of the functions at those locations are sent, never the error message, the page address or the browser identification. A request from the window to the app that fails is reported as `InvokeError` with the fixed name of the request (for example `start_port_forward_tcp_cmd`), never its arguments or the message. Reports never include cluster names, namespaces, service names, aliases, kubeconfig files, log lines or your host name.

Backend operations classified as `cancelled` leave an informational breadcrumb instead of a separate error report. With performance data enabled, their traces use `cancelled` rather than `internal_error`. A cancelled start whose rollback could not finish is still reported, as `cleanup_incomplete`, because it can leave a relay pod, loopback alias or hosts entry behind.

Reports go to a GlitchTip server run by the kftray maintainer at `glitchtip.cavarsa.app`. The proxy in front of it drops the client IP before the report is stored. Setting the `DO_NOT_TRACK` environment variable turns reporting off whatever the setting says, and development builds never send reports.

## Report a problem

Open the footer menu and choose `Report a problem` to describe a problem, with or without a crash. Enter a description of up to 4000 characters. The email field is optional and lets the maintainer contact you.

To describe a specific failure, choose `Report` on its notification. The option appears for individual failed app requests captured while crash reports are enabled.

Sending applies to that report only and leaves your diagnostics settings unchanged. You can send a report with crash reports and performance data turned off. Development builds and sessions started with `DO_NOT_TRACK` cannot send reports.

Reports go to the same server as crash reports and carry your description, optional email, app version, build target, submission time and a random report id. A report about a specific failure also references that error. Logs, configuration files and screenshots are never attached. Review your text before sending and leave out passwords, tokens, kubeconfig contents and cluster details.

Each report is stored as its own entry, so it is never lost to a timing issue on the server. If submission fails, the form keeps your text so you can try again or cancel.

## Performance data

Performance data is the second switch in the same dialog. It starts off and is a separate choice from crash reports. You can change it under **Settings > Performance Data**.

When it's on, kftray records how long it takes to start a port forward (including the ones started automatically on launch), stop one or stop all of them, to start up (opening the database, running migrations, reading settings), to import configurations from a git repository and to start the MCP server. It also records whether the operation failed, and how long each step inside it took (for example allocating the local address, connecting to the cluster, binding the local port, deleting the relay pod). The window records how long each of its requests to the app takes, by the fixed name of the request (for example `get_configs`); the request's arguments and result are never sent. Each record has the operation or request name, its step names and durations, its result (and the fixed failure reason described above), the app version, the build target and the run id described above. It never includes cluster names, namespaces, service names, aliases, kubeconfig files or your host name.

The data goes to the same GlitchTip server as crash reports. `DO_NOT_TRACK` and development builds turn it off too.

Turning performance data off stops new timing records for backend operations, including operations still running. A capture already in progress finishes before the setting change returns. Records already queued for delivery can still be sent. Crash reports keep their separate setting.

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
