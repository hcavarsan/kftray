# KFtui Usage Guide

Start with a simple configuration file. Create `config.json`:

```json
[
  {
    "alias": "my-api",
    "context": "minikube",
    "namespace": "default",
    "service": "api-service",
    "local_port": 8080,
    "remote_port": 80,
    "protocol": "tcp",
    "workload_type": "service"
  }
]
```

Launch kftui with this configuration:

```bash
kftui --configs-path config.json
```

The interface displays your configuration in the "Stopped" table on the left. Press `f` to start the port-forward. The configuration moves to the "Running" table on the right, and your service becomes accessible at `localhost:8080`. Press `f` again to stop it.

## Persistent Storage

To avoid specifying the config file path repeatedly, save configurations to kftui's database:

```bash
kftui --configs-path config.json --save
```

After saving, run `kftui` without arguments to access your stored configurations. This approach works well for frequently used port-forward setups.

## Configuration Sources

kftui supports several ways to load configurations, each suited to different workflows.

### Local Files

Load configurations from local JSON files:

```bash
kftui --configs-path /path/to/config.json
```

### GitHub Repositories

Import configurations from version-controlled repositories:

```bash
kftui --github-url https://github.com/your-team/k8s-configs --configs-path environments/dev.json
```

This method works well for teams that maintain environment-specific configurations in version control.

### Inline JSON

Pass JSON configuration directly for scripting scenarios:

```bash
kftui --json '[{"alias":"test","namespace":"default","service":"my-service","local_port":8080,"remote_port":80,"protocol":"tcp","workload_type":"service"}]'
```

### Standard Input

Read configurations from stdin for pipeline integration:

```bash
echo '[{"alias":"api",...}]' | kftui --stdin
```

### Command-Line Options

**`--save`**: Store configurations in the database for future use
**`--flush`**: Clear existing configurations before importing new ones
**`--auto-start`**: Start all port-forwards immediately after loading
**`--non-interactive`**: Run without the interface for automation scripts

## Service Auto-Discovery

Rather than manually creating configuration files, kftui can discover services through Kubernetes annotations. This approach keeps port-forward configurations synchronized with service definitions.

Add annotations to your Kubernetes services:

```yaml
apiVersion: v1
kind: Service
metadata:
  name: monitoring-stack
  namespace: observability
  annotations:
    kftray.app/configs: "grafana-3000-3000,prometheus-9090-9090,alertmanager-9093-9093"
spec:
  selector:
    app: monitoring
  ports:
    - name: grafana
      port: 3000
    - name: prometheus
      port: 9090
    - name: alertmanager
      port: 9093
```

The annotation format is `alias-local_port-remote_port`. Multiple configurations are separated by commas.

To use auto-discovery, navigate to the top menu in kftui and select "Auto Add". Choose your Kubernetes context from the available options, and kftui will create configurations for all annotated services in that context.

## Interface Organization

The interface consists of four main areas:

**Top menu bar**: Contains Help, Auto Add, Import, Export, Settings, About, and Exit options
**Stopped configurations** (left): Displays configurations ready to start
**Running configurations** (right): Shows active port-forwards with status information
**Details panel** (bottom): Provides information about the selected configuration

### Menu Navigation

The top menu provides seven functions:

- **Help**: Display usage information and keyboard shortcuts
- **Auto Add**: Discover services from your Kubernetes cluster
- **Import**: Load configuration files through a file browser
- **Export**: Save current configurations to a JSON file
- **Settings**: Configure application behavior
- **About**: Show version and project information
- **Exit**: Stop all active port-forwards and close the application

Navigate menu items with `←/→` arrow keys and press `Enter` to select.

## Application Settings

Access settings by pressing `s` or selecting Settings from the menu.

**Disconnect Timeout**: Configure automatic disconnection after a specified period of inactivity. Set to 0 to disable automatic timeouts.

**Network Monitor**: Enable or disable network connectivity monitoring. When enabled, kftui monitors network status and attempts to reconnect dropped port-forwards.

Settings persist between application sessions.

## HTTP Request Logging

kftui provides HTTP request and response logging for debugging network traffic through port-forwards.

Enable logging by setting `"http_logs_enabled": true` in your configuration file, or toggle it for existing configurations by pressing `l` in the interface.

### HTTP Logs Viewer

Press `V` to open the HTTP logs viewer, which operates in two modes:

**List Mode** (default view):

- Displays all HTTP requests with timestamps and status codes
- Navigate requests with `↑/↓` arrow keys
- Use `PageUp/PageDown` to scroll through multiple entries
- Press `Enter` on any request to view detailed information
- Press `a` to toggle automatic scrolling for new requests

**Detail Mode** (activated by pressing Enter on a request):

- Shows complete request and response information including headers and body content
- Scroll through details with `↑/↓` arrow keys
- Use `PageUp/PageDown` for faster navigation
- Press `r` to replay the selected request
- Press `Esc` to return to list mode

### HTTP Logs Configuration

Press `L` to configure HTTP logging behavior:

- Enable or disable logging for individual configurations
- Set maximum log file size (1-1000 MB)
- Configure retention period (1-365 days)
- Enable automatic cleanup of old log files

## Complete Keyboard Reference

| Key | Function |
|-----|----------|
| `f` | Start or stop port-forward |
| `i` | Import configurations |
| `e` | Export configurations |
| `d` | Delete selected configurations |
| `a` | Select all configurations |
| `Space` | Toggle individual selection |
| `Tab` | Switch between interface components |
| `←/→` | Navigate menu items |
| `s` | Open settings |
| `h` | Show help information |
| `q` | Show about information |
| `l` | Toggle HTTP logging |
| `L` | Configure HTTP logging |
| `V` | View HTTP logs |
| `o` | Open HTTP logs in external editor |
| `↑/↓` | Navigate within sections |
| `PageUp/PageDown` | Scroll through content |
| `Home/End` | Jump to first/last item |
| `Ctrl+C` | Exit application |

## Configuration File Structure

Configuration files use JSON format with the following fields:

```json
{
  "alias": "my-service",           // Display name in interface
  "namespace": "production",       // Kubernetes namespace (required)
  "service": "api-service",        // Target service name
  "local_port": 8080,             // Local binding port
  "remote_port": 80,              // Remote service port
  "protocol": "tcp",              // Network protocol (required)
  "workload_type": "service",     // Target type: "service" or "pod"
  "context": "prod-cluster",      // Kubernetes context
  "kubeconfig": "/path/to/config", // Kubeconfig file path
  "http_logs_enabled": true       // Enable HTTP logging
}
```

## What kftui Can Do

kftui handles pretty much everything the kftray GUI does, just through terminal commands and keyboard shortcuts. The auto-discovery stuff saves time by reading your Kubernetes service annotations instead of making you type configs manually. The HTTP logging is useful for debugging API calls and seeing what requests are going through your port-forwards.

If you work mostly in terminals or need to manage port-forwards over SSH, kftui covers everything without needing a desktop environment.
