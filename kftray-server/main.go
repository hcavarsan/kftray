package main

import (
	"encoding/binary"
	"io"
	"log"
	"net"
	"os"
	"strconv"

	"github.com/miladrahimi/gorelay"
)

func main() {
	targetHost := os.Getenv("REMOTE_ADDRESS")
	targetPortStr := os.Getenv("REMOTE_PORT")
	proxyPortStr := os.Getenv("LOCAL_PORT")
	proxyType := os.Getenv("PROXY_TYPE")

	log.Printf("Configured REMOTE_ADDRESS: %s", targetHost)
	log.Printf("Configured REMOTE_PORT: %s", targetPortStr)
	log.Printf("Configured LOCAL_PORT: %s", proxyPortStr)
	log.Printf("Configured PROXY_TYPE: %s", proxyType)

	targetPort, err := strconv.Atoi(targetPortStr)
	if err != nil {
		log.Fatalf("Invalid REMOTE_PORT: %s", targetPortStr)
	}

	proxyPort, err := strconv.Atoi(proxyPortStr)
	if err != nil {
		log.Fatalf("Invalid LOCAL_PORT: %s", proxyPortStr)
	}

	switch proxyType {
	case "tcp":
		tcp := gorelay.NewTcpRelay()
		err := tcp.Relay(proxyPort, targetPort, targetHost)
		if err != nil {
			log.Fatalf("Failed to start the TCP proxy server: %s", err)
		}
	case "udp":
		startUDPOverTCPProxy(targetHost, targetPort, proxyPort)
	default:
		log.Fatalf("Unsupported PROXY_TYPE: %s", proxyType)
	}
}

func startUDPOverTCPProxy(targetHost string, targetPort, proxyPort int) {
	listener, err := net.Listen("tcp", ":"+strconv.Itoa(proxyPort))
	if err != nil {
		log.Fatalf("Failed to start TCP listener: %s", err)
	}
	defer listener.Close()

	log.Printf("UDP over TCP proxy listening on port %d", proxyPort)

	for {
		conn, err := listener.Accept()
		if err != nil {
			log.Printf("Failed to accept connection: %s", err)
			continue
		}
		go handleTCPConnection(conn, targetHost, targetPort)
	}
}

func handleTCPConnection(conn net.Conn, targetHost string, targetPort int) {
	defer conn.Close()
	clientAddr := conn.RemoteAddr().String()
	log.Printf("Accepted TCP connection from %s", clientAddr)

	udpAddr, err := net.ResolveUDPAddr("udp", net.JoinHostPort(targetHost, strconv.Itoa(targetPort)))
	if err != nil {
		log.Printf("[%s] Failed to resolve UDP address: %s", clientAddr, err)
		return
	}

	udpConn, err := net.DialUDP("udp", nil, udpAddr)
	if err != nil {
		log.Printf("[%s] Failed to dial UDP: %s", clientAddr, err)
		return
	}
	defer udpConn.Close()
	log.Printf("[%s] Established UDP connection to %s", clientAddr, udpAddr)

	// Forward TCP to UDP
	go func() {
		defer udpConn.Close()
		for {
			// Read the length of the UDP packet from the TCP stream
			var lengthBytes [4]byte
			_, err := io.ReadFull(conn, lengthBytes[:])
			if err != nil {
				if err != io.EOF {
					log.Printf("[%s] Error reading packet length from TCP: %s", clientAddr, err)
				}
				return
			}
			length := binary.BigEndian.Uint32(lengthBytes[:])

			buf := make([]byte, length) // Create a buffer with the exact packet size
			_, err = io.ReadFull(conn, buf)
			if err != nil {
				log.Printf("[%s] Error reading from TCP: %s", clientAddr, err)
				return
			}
			log.Printf("[%s] TCP -> UDP: %x", clientAddr, buf)

			_, err = udpConn.Write(buf)
			if err != nil {
				log.Printf("[%s] Error writing to UDP: %s", clientAddr, err)
				return
			}
		}
	}()

	// Forward UDP to TCP
	buf := make([]byte, 65535) // UDP max packet size
	for {
		n, _, err := udpConn.ReadFromUDP(buf)
		if err != nil {
			log.Printf("[%s] Error reading from UDP: %s", clientAddr, err)
			return
		}
		log.Printf("[%s] UDP -> TCP: %x", clientAddr, buf[:n])

		// Prepend the length of the UDP packet to the data sent over TCP
		lengthBytes := make([]byte, 4)
		binary.BigEndian.PutUint32(lengthBytes, uint32(n))
		_, err = conn.Write(lengthBytes)
		if err != nil {
			log.Printf("[%s] Error writing packet length to TCP: %s", clientAddr, err)
			return
		}

		_, err = conn.Write(buf[:n])
		if err != nil {
			log.Printf("[%s] Error writing to TCP: %s", clientAddr, err)
			return
		}
	}
}
