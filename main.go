package main

import (
	"bufio"
	"encoding/json"
	"fmt"
	"io/ioutil"
	"log"
	"os"
	"os/exec"
	"strings"
	"sync"
	"time"

	"fyne.io/fyne/v2"
	"fyne.io/fyne/v2/app"
	"fyne.io/fyne/v2/container"
	"fyne.io/fyne/v2/driver/desktop"
	"fyne.io/fyne/v2/widget"
	"fyne.io/systray"

	_ "embed"
)

type Configs []struct {
	Namespace  string `json:"namespace"`
	Deployment string `json:"deployment"`
	LocalPort  string `json:"localPort"`
	RemotePort string `json:"remotePort"`
	Kubeconfig string `json:"kubeconfig"`
}

type Config struct {
	Namespace  string `json:"namespace"`
	Deployment string `json:"deployment"`
	LocalPort  string `json:"localPort"`
	RemotePort string `json:"remotePort"`
	Kubeconfig string `json:"kubeconfig"`
}

var (
	//go:embed img/icon.png
	statusIconActive      []byte
	stopCh                []chan struct{}
	mutex                 sync.Mutex
	configStatus          []string
	statusStringBuilder   strings.Builder
	configs               Configs
	startButtontray       *fyne.MenuItem
	fyneapp               fyne.App
	cmdInstances          []*exec.Cmd
	startButton           *widget.Button
	infoLabel             *widget.Label
	uiUpdateCh            = make(chan func())
	statusTexts           []string
	err                   error
	window                fyne.Window
	menuItems             []*fyne.MenuItem
	portForwardingStarted bool
	mainMenuItem          *fyne.MenuItem
)

const startPortForwardingText = "Start Port Forward"
const stopPortForwardingText = "Stop Port Forward"

func main() {
	userHome := os.Getenv("HOME")
	kftrayConfig := os.Getenv("KFTRAY_CONFIG")
	if kftrayConfig == "" {
		kftrayConfig = fmt.Sprintf("%s/.kftray/config.json", userHome)
	}

	fyneapp = app.NewWithID("KFTray")
	window = fyneapp.NewWindow("KFTray")

	startButton = widget.NewButton(startPortForwardingText, togglePortForwarding)
	mainMenuItem = fyne.NewMenuItem(startPortForwardingText, togglePortForwarding)

	configs, err = readConfigFromFile("config.json")
	if err != nil {
		fmt.Println("Error reading configuration:", err)
		return
	}

	configStatus = make([]string, len(configs))
	for i, config := range configs {
		statusText := fmt.Sprintf("Port Forward Stopped for Pod: %s, Namespace: %s, Local Port: %s, Remote Port: %s",
			config.Deployment, config.Namespace, config.LocalPort, config.RemotePort)
		statusTexts = append(statusTexts, statusText)
		configStatus[i] = statusText
		menuItem := fyne.NewMenuItem(statusText, nil)
		menuItems = append(menuItems, menuItem)
	}

	// Create a new menu item for each status text
	quitButton := widget.NewButton("Quit", func() {
		fyneapp.Quit()
	})

	infoLabel = widget.NewLabel("Initial Text")
	window.SetContent(container.NewVBox(
		startButton,
		infoLabel,
		quitButton,
	))
	initSystemTray(menuItems)

	window.ShowAndRun()

}

func updateTrayMenu(configs Configs) {
	statusTexts = []string{}
	menuItems = []*fyne.MenuItem{}
	if portForwardingRunning() {
		mainMenuItem = fyne.NewMenuItem(startPortForwardingText, togglePortForwarding)
		systray.SetTitle("KFTray - Port Forward Stopped")
		for i, config := range configs {
			startButton.SetText(startPortForwardingText)
			statusText := fmt.Sprintf("Port Forward Stopped for Pod: %s, Namespace: %s, Local Port: %s, Remote Port: %s",
				config.Deployment, config.Namespace, config.LocalPort, config.RemotePort)
			statusTexts = append(statusTexts, statusText)
			configStatus[i] = statusText

			// Create a new menu item for each status text
			menuItem := fyne.NewMenuItem(statusText, nil)
			menuItems = append(menuItems, menuItem)
		}
		fyne.CurrentApp().SendNotification(fyne.NewNotification("KFTray", "Port Forward Stopped"))

	} else {
		mainMenuItem = fyne.NewMenuItem(stopPortForwardingText, togglePortForwarding)
		systray.SetTitle("KFTray - Port Forward Started")
		for i, config := range configs {
			startButton.SetText(stopPortForwardingText)
			statusText := fmt.Sprintf("Port Forward Started for Pod: %s, Namespace: %s, Local Port: %s, Remote Port: %s",
				config.Deployment, config.Namespace, config.LocalPort, config.RemotePort)
			statusTexts = append(statusTexts, statusText)
			configStatus[i] = statusText
			menuItem := fyne.NewMenuItem(statusText, nil)
			menuItems = append(menuItems, menuItem)
		}
		fyne.CurrentApp().SendNotification(fyne.NewNotification("KFTray", "Port Forward Started"))

	}
	initSystemTray(menuItems)

}

func initSystemTray(menuItems []*fyne.MenuItem) {
	icon := fyne.NewStaticResource("icon", statusIconActive)
	if desk, ok := fyneapp.Driver().(desktop.App); ok {
		infoLabel.SetText(strings.Join(statusTexts, "\n"))

		menuItems = append([]*fyne.MenuItem{mainMenuItem, fyne.NewMenuItemSeparator()}, menuItems...)

		// Add a separator between the status items and the "Quit" button
		menuItems = append(menuItems, fyne.NewMenuItemSeparator())

		menuItems = append(menuItems, fyne.NewMenuItem("Quit", func() {
			fyneapp.Quit()
		}))

		m := fyne.NewMenu("", menuItems...)

		desk.SetSystemTrayIcon(icon)
		desk.SetSystemTrayMenu(m)
	}
}

func togglePortForwarding() {

	if portForwardingRunning() {
		log.Printf("Port Forward Stopping: %v", portForwardingRunning())
		stopPortForwarding(configs)
		updateTrayMenu(configs)
	} else {
		if !portForwardingRunning() {
			log.Printf("Port Forward Starting: %v", portForwardingRunning())
			startPortForwarding(configs)
			updateTrayMenu(configs)
		}
	}

}
func startPortForwarding(configs Configs) bool {
	stopCh = make([]chan struct{}, len(configs))
	if portForwardingRunning() {
		fmt.Println("Port Forward is already running")
		return false
	}

	wg := sync.WaitGroup{}
	wg.Add(len(configs))

	mutex.Lock()
	for _, item := range menuItems {
		item.Disabled = true
	}
	startButton.Disable()
	systray.SetTitle("KFTray - please wait...")

	mutex.Unlock()

	for i, config := range configs {
		stopCh[i] = make(chan struct{})
		cmd := createPortForwardCommand(config)
		processExited := make(chan bool)

		go func(cmdCopy *exec.Cmd, stopChan chan struct{}, configCopy Config, processExited chan bool) {
			handlePortForwarding(cmdCopy, stopChan, configCopy, processExited)
			wg.Done() // Decrements the WaitGroup counter when function done.
		}(cmd, stopCh[i], config, processExited)

	}

	wg.Wait()
	go func() {
		// Waits here for all go routines to get done.
		mutex.Lock()
		time.Sleep(2)

		startButton.Enable()
		for _, item := range menuItems {
			item.Disabled = false
		}
		mutex.Unlock()
	}()

	return true
}

func createPortForwardCommand(config Config) *exec.Cmd {
	cmd := exec.Command(
		"kubectl",
		"port-forward",
		fmt.Sprintf("deploy/%s", config.Deployment),
		fmt.Sprintf("%s:%s", config.LocalPort, config.RemotePort),
		fmt.Sprintf("--namespace=%s", config.Namespace),
	)
	cmdInstances = append(cmdInstances, cmd) // store the cmd instance
	return cmd
}

func handlePortForwarding(cmd *exec.Cmd, stopCh chan struct{}, config Config, processExited chan<- bool) bool { // add a new parameter

	stdout, err := cmd.StdoutPipe()
	if err != nil {
		fmt.Println("Error creating stdout pipe:", err)
		close(stopCh)
		return false
	}
	err = cmd.Start()

	if err != nil {
		fmt.Println("Error starting port-forwarding:", err)
		close(stopCh)
		return false
	}

	scanner := bufio.NewScanner(stdout)
	for {
		for {
			select {
			case <-stopCh:
				if cmd.ProcessState != nil && !cmd.ProcessState.Exited() {
					if err := cmd.Process.Kill(); err != nil {
						log.Println("Failed to kill process: ", err)
					}
					if err := cmd.Process.Release(); err != nil {
						log.Println("Failed to release process: ", err)
					}
				}
				portForwardingStarted = false
				cmdInstances = nil
				return portForwardingStarted

			default:
				if scanner.Scan() {
					line := scanner.Text()
					if strings.Contains(line, "Forwarding from 127.0.0.1") {
						log.Println(line)
						log.Println("Port Forward Started")
					}
					portForwardingStarted = true
					return portForwardingStarted
				}
			}
		}
	}
}
func stopPortForwarding(configs Configs) bool {
	wg := sync.WaitGroup{}
	wg.Add(len(configs))

	for _, item := range menuItems {
		item.Disabled = true
	}
	for _, cmd := range cmdInstances {
		go func(cmdCopy *exec.Cmd) { // goroutine for each stopping process
			defer wg.Done() // Ensure wg.Done() is always called
			if cmdCopy.ProcessState != nil && cmdCopy.ProcessState.Exited() {
				log.Println("Process has already exited")
				return
			}
			if err := cmdCopy.Process.Kill(); err != nil {
				log.Println("Failed to kill process: ", err)
			}
			if err := cmdCopy.Process.Release(); err != nil {
				log.Println("Failed to release process: ", err)
			}
		}(cmd)
	}
	startButton.Disable()
	systray.SetTitle("KFTray - please wait...")
	time.Sleep(2 * time.Second)
	wg.Wait()
	go func() {
		mutex.Lock()
		startButton.Enable()
		mutex.Unlock()
	}()
	cmdInstances = nil
	return true
}
func portForwardingRunning() bool {
	for _, cs := range configStatus {
		if strings.Contains(cs, "Port Forward Started") {
			return true
		}
	}
	return false
}

func readConfigFromFile(filename string) (Configs, error) {
	var configs Configs

	file, err := os.Open(filename)
	if err != nil {
		return configs, err
	}
	defer file.Close()
	input, _ := ioutil.ReadFile(filename)
	json.Unmarshal(input, &configs)
	if err != nil {
		return configs, err
	}

	return configs, nil
}

func findConfigIndex(config Config) int {
	for i, cs := range configStatus {
		if strings.Contains(cs, fmt.Sprintf("Pod: %s, Namespace: %s, Local Port: %s, Remote Port: %s",
			config.Deployment, config.Namespace, config.LocalPort, config.RemotePort)) {
			return i
		}
	}
	return -1
}
