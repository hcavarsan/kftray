package tray

import (
	"log"
	"os/exec"
	"strings"
	"sync"

	"fyne.io/fyne/v2"
	"fyne.io/fyne/v2/driver/desktop"
	"fyne.io/fyne/v2/widget"
	"fyne.io/systray"

	"github.com/hcavarsan/kftray/config"
	"github.com/hcavarsan/kftray/portforward"
)

type TrayPackage struct {
	stopCh              []chan struct{}
	mutex               sync.Mutex
	configStatus        []string
	statusStringBuilder strings.Builder
	startButtontray     *fyne.MenuItem
	cmdInstances        []*exec.Cmd
	startButton         *widget.Button
	uiUpdateCh          chan func()
	statusTexts         []string
	err                 error
	mainMenuItem        *fyne.MenuItem
	menuItems           []*fyne.MenuItem
}

const StartPortForwardingText = "Start Port Forwarding"
const StopPortForwardingText = "Stop Port Forwarding"

var configs = []config.Config{}

func (tp *TrayPackage) UpdateTrayMenu(fyneapp fyne.App) {
	listConfig := config.GetConfigs()
	tp.statusTexts = config.GetConfigStatus()

	isPortForwardingRunning := portforward.PortForwardingRunning()

	if !isPortForwardingRunning {
		log.Printf("Port Forward Starting: %v", isPortForwardingRunning)
		portforward.StartPortForwarding(listConfig)
		systray.SetTitle("KFTray - Port Forward Started")
		fyneapp.SendNotification(fyne.NewNotification("KFTray", "Port Forward Started"))
		tp.menuItems = config.GetMenuStarted()
		config.MainMenuItem = fyne.NewMenuItem(StopPortForwardingText, func() { tp.UpdateTrayMenu(fyneapp) })
	} else {
		log.Printf("Port Forward Stopping: %v", isPortForwardingRunning)
		portforward.StopPortForwarding(listConfig)
		fyneapp.SendNotification(fyne.NewNotification("KFTray", "Port Forward Stopped"))
		systray.SetTitle("KFTray - Port Forward Stopped")
		tp.menuItems = config.GetMenuStopped()
		config.MainMenuItem = fyne.NewMenuItem(StartPortForwardingText, func() { tp.UpdateTrayMenu(fyneapp) })
	}

	tp.InitSystemTray(tp.menuItems, config.MainMenuItem, config.StartButton, config.InfoLabel, fyneapp)
}

func (tp *TrayPackage) InitSystemTray(menuItems []*fyne.MenuItem, mainMenuItem *fyne.MenuItem, startButton *widget.Button, infoLabel *widget.Label, fyneapp fyne.App) {
	if desk, ok := fyneapp.Driver().(desktop.App); ok {
		if infoLabel == nil {
			infoLabel = widget.NewLabel("")
		}

		infoLabel.SetText(strings.Join(tp.statusTexts, "\n"))

		menuItems = append([]*fyne.MenuItem{mainMenuItem, fyne.NewMenuItemSeparator()}, menuItems...)
		menuItems = append(menuItems,
			fyne.NewMenuItemSeparator(),
			fyne.NewMenuItem("Quit", func() { fyneapp.Quit() }),
		)

		m := fyne.NewMenu("", menuItems...)

		desk.SetSystemTrayIcon(config.Icon)
		desk.SetSystemTrayMenu(m)
	}
	fyneapp.Run()
}
