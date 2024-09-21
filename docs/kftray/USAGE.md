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
   - Set the local and remote port numbers
   - Configure a custom local IP address (optional)

4. **Activate Your Configuration**: With your configuration saved, simply click on the switch button in the main menu to start the port forward in a single por forward or in Start All to start all configurations at the same time

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
3. Enter the URL of your Git repository and path containing the JSON file
4. If your GitHub repository is private, you will need to enter the private token. Credentials are securely saved in the SO keyring (Keychain on macOS). Kftray does not store or save credentials in any local file; they are only stored in the local keyring.
5. Select the polling time for when Kftray will synchronize configurations and retrieve them from GitHub.

6. KFtray will now sync with the Git repository to automatically import any new configurations or changes committed to the JSON file.

This allows you to quickly deploy any port forward changes to all team members. And if someone on your team adds a new configuration, it will be automatically synced to everyone else's KFtray.
