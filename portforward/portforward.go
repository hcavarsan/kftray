package portforward

import (
	"bufio"
	"fmt"
	"log"
	"os/exec"
	"strings"
	"sync"
	"time"

	"fyne.io/systray"
	"github.com/hcavarsan/kftray/config"
)

var cmdInstances []*exec.Cmd
var mutex sync.Mutex

func StartPortForwarding(configs config.Configs) bool {
	stopCh := make([]chan struct{}, len(configs))
	if PortForwardingRunning() {
		fmt.Println("Port Forward is already running")
		return false
	}

	wg := sync.WaitGroup{}
	wg.Add(len(configs))

	systray.SetTitle("KFTray - please wait...")

	for i, cfg := range configs {
		stopCh[i] = make(chan struct{})
		cmd := CreatePortForwardCommand(cfg)
		processExited := make(chan bool)

		go func(cmdCopy *exec.Cmd, stopChan chan struct{}, configCopy config.Config, processExited chan bool) {
			HandlePortForwarding(cmdCopy, stopChan, configCopy, processExited)
			wg.Done() // Decrements the WaitGroup counter when function done.
		}(cmd, stopCh[i], cfg, processExited)

	}

	wg.Wait()
	go func() {
		// Waits here for all go routines to get done.
		mutex.Lock()
		time.Sleep(2)
		mutex.Unlock()
	}()

	return true
}

func CreatePortForwardCommand(config config.Config) *exec.Cmd {
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

func HandlePortForwarding(cmd *exec.Cmd, stopCh chan struct{}, config config.Config, processExited chan<- bool) bool { // add a new parameter

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

				cmdInstances = nil
				return false

			default:
				if scanner.Scan() {
					line := scanner.Text()
					if strings.Contains(line, "Forwarding from 127.0.0.1") {
						log.Println(line)
						log.Println("Port Forward Started")
					}
					return true
				}
			}
		}
	}
}
func StopPortForwarding(configs config.Configs) bool {
	wg := sync.WaitGroup{}
	wg.Add(len(configs))

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
	time.Sleep(2 * time.Second)
	wg.Wait()

	cmdInstances = nil
	return true
}

func PortForwardingRunning() bool {
	for _, cmd := range cmdInstances {
		// If cmd.ProcessState is nil or process has not exited yet, return true.
		if cmd.ProcessState == nil || !cmd.ProcessState.Exited() {
			return true
		}
	}
	return false
}
