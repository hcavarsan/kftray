package config

import (
	"encoding/json"
	"fmt"
	"io/ioutil"
	"os"
	"os/exec"
	"strings"
	"sync"

	"fyne.io/fyne/v2"
	"fyne.io/fyne/v2/app"
	"fyne.io/fyne/v2/widget"

	_ "embed"
)

type Config struct {
	Namespace  string `json:"namespace"`
	Deployment string `json:"deployment"`
	LocalPort  string `json:"localPort"`
	RemotePort string `json:"remotePort"`
	Kubeconfig string `json:"kubeconfig"`
}

var (
	stopCh              []chan struct{}
	mutex               sync.Mutex
	ConfigStatus        []string
	statusStringBuilder strings.Builder
	startButtontray     *fyne.MenuItem
	cmdInstances        []*exec.Cmd
	StartButton         *widget.Button
	InfoLabel           *widget.Label
	uiUpdateCh          = make(chan func())
	StatusTexts         []string
	menuItems           []*fyne.MenuItem
	MainMenuItem        *fyne.MenuItem
	TenuItems           []*fyne.MenuItem
)

//go:embed icon.png
var statusIconActive []byte
var Icon = fyne.NewStaticResource("icon", statusIconActive)

type Configs []Config
type MenuItems []*fyne.MenuItem

var Fyneapp fyne.App
var Window fyne.Window

func InitConfigs() fyne.App {
	Fyneapp = app.NewWithID("KFTray")

	return Fyneapp
}

func ReadConfigFromFile(filename string) (Configs, error) {
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

func GetConfigStatus() []string {
	configs, err := ReadConfigFromFile("config.json")
	if err != nil {
		fmt.Println("Error reading configuration:", err)
		return []string{}
	}
	for i, config := range configs {
		statusText := fmt.Sprintf("Port Forward Stopped for Pod: %s, Namespace: %s, Local Port: %s, Remote Port: %s",
			config.Deployment, config.Namespace, config.LocalPort, config.RemotePort)
		StatusTexts = append(StatusTexts, statusText)
		ConfigStatus[i] = statusText
	}
	return ConfigStatus
}

func GetConfigs() Configs {
	configs, err := ReadConfigFromFile("config.json")
	if err != nil {
		fmt.Println("Error reading configuration:", err)
		return configs
	}

	return configs
}

func GetMenuStarted() MenuItems {
	configs, err := ReadConfigFromFile("config.json")
	if err != nil {
		fmt.Println("Error reading configuration:", err)
		return nil
	}
	menuItems := []*fyne.MenuItem{}
	StatusTexts = []string{}
	for _, config := range configs {
		statusText := fmt.Sprintf("Port Forward Started for Pod: %s, Namespace: %s, Local Port: %s, Remote Port: %s",
			config.Deployment, config.Namespace, config.LocalPort, config.RemotePort)
		StatusTexts = append(StatusTexts, statusText)
		ConfigStatus = append(ConfigStatus, statusText)
		menuItem := fyne.NewMenuItem(statusText, nil)
		menuItems = append(menuItems, menuItem)
	}

	return menuItems
}

func GetMenuStopped() MenuItems {
	configs, err := ReadConfigFromFile("config.json")
	if err != nil {
		fmt.Println("Error reading configuration:", err)
		return nil
	}
	menuItems := []*fyne.MenuItem{}
	StatusTexts = []string{}
	for _, config := range configs {
		statusText := fmt.Sprintf("Port Forward Stopped for Pod: %s, Namespace: %s, Local Port: %s, Remote Port: %s",
			config.Deployment, config.Namespace, config.LocalPort, config.RemotePort)
		StatusTexts = append(StatusTexts, statusText)
		ConfigStatus = append(ConfigStatus, statusText)
		menuItem := fyne.NewMenuItem(statusText, nil)
		menuItems = append(menuItems, menuItem)
	}

	return menuItems
}
