package main

import (
	"fmt"
	"os"

	"fyne.io/fyne/v2"
	"fyne.io/fyne/v2/driver/desktop"
	"fyne.io/fyne/v2/widget"

	"github.com/hcavarsan/kftray/config"
	"github.com/hcavarsan/kftray/tray"
)

const StartPortForwardingText = "Start Port Forward"
const StopPortForwardingText = "Stop Port Forward"

func main() {
	// Get user home and config path
	userHome := os.Getenv("HOME")
	kftrayConfig := os.Getenv("KFTRAY_CONFIG")

	if kftrayConfig == "" {
		kftrayConfig = fmt.Sprintf("%s/.kftray/config.json", userHome)
	}
	tp := tray.TrayPackage{}
	// Get menu items and status texts based on the configuration

	// Initialize Configurations and GUI window
	fyneapp := config.InitConfigs()

	// Creating start button and setting its onClick handler
	config.StartButton = widget.NewButton(StartPortForwardingText, func() { tp.UpdateTrayMenu(fyneapp) })

	// Creating main menu item and setting its onClick handler
	config.MainMenuItem = fyne.NewMenuItem(StartPortForwardingText, func() { tp.UpdateTrayMenu(fyneapp) })

	config.TenuItems = config.GetMenuStopped()

	if desk, ok := fyneapp.Driver().(desktop.App); ok {
		m := fyne.NewMenu(StartPortForwardingText,
			config.MainMenuItem,
			fyne.NewMenuItemSeparator(),
		)
		desk.SetSystemTrayIcon(config.Icon)

		desk.SetSystemTrayMenu(m)
	}
	fyneapp.Run()
}
