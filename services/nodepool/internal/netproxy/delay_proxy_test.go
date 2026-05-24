package netproxy

import (
	"io"
	"net"
	"testing"
	"time"
)

func freePort(t *testing.T) int {
	t.Helper()
	ln, err := net.Listen("tcp", "127.0.0.1:0")
	if err != nil {
		t.Fatalf("listen free port: %v", err)
	}
	defer ln.Close()
	return ln.Addr().(*net.TCPAddr).Port
}

func startEchoServer(t *testing.T) (port int, stop func()) {
	t.Helper()
	ln, err := net.Listen("tcp", "127.0.0.1:0")
	if err != nil {
		t.Fatalf("listen echo: %v", err)
	}
	done := make(chan struct{})
	go func() {
		defer close(done)
		for {
			conn, err := ln.Accept()
			if err != nil {
				return
			}
			go func(c net.Conn) {
				defer c.Close()
				_, _ = io.Copy(c, c)
			}(conn)
		}
	}()
	return ln.Addr().(*net.TCPAddr).Port, func() {
		_ = ln.Close()
		<-done
	}
}

func TestDelayProxyAcceptsConnectionAfterIdleTimeout(t *testing.T) {
	targetPort, stopEcho := startEchoServer(t)
	defer stopEcho()

	proxy := NewDelayProxy(DelayProxyConfig{
		Name:       "test-proxy",
		ListenHost: "127.0.0.1",
		ListenPort: freePort(t),
		TargetHost: "127.0.0.1",
		TargetPort: targetPort,
	})
	if err := proxy.Start(); err != nil {
		t.Fatalf("proxy start: %v", err)
	}
	defer proxy.Stop()

	time.Sleep(800 * time.Millisecond)

	conn, err := net.DialTimeout("tcp", proxy.ListenAddr(), 2*time.Second)
	if err != nil {
		t.Fatalf("dial proxy: %v", err)
	}
	defer conn.Close()
	_ = conn.SetDeadline(time.Now().Add(2 * time.Second))
	if _, err := conn.Write([]byte("ping")); err != nil {
		t.Fatalf("write ping: %v", err)
	}
	buf := make([]byte, 4)
	if _, err := io.ReadFull(conn, buf); err != nil {
		t.Fatalf("read echo: %v", err)
	}
	if string(buf) != "ping" {
		t.Fatalf("echo=%q, want ping", string(buf))
	}
	if proxy.Connections() != 1 {
		t.Fatalf("connections=%d, want 1", proxy.Connections())
	}
}
