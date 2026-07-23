//go:build windows

package main

//#include "errno.h"
import "C"

import (
	"context"
	"fmt"
	"io"
	"net"
	"os"
	"sync"
	"unsafe"

	"tailscale.com/hostinfo"
	"tailscale.com/tsnet"
)

func main() {}

var servers struct {
	mu   sync.Mutex
	next C.int
	m    map[C.int]*server
}

type server struct {
	s       *tsnet.Server
	lastErr string
	started bool
	forward []net.Listener
}

func getServer(sd C.int) *server {
	servers.mu.Lock()
	defer servers.mu.Unlock()
	return servers.m[sd]
}

func (s *server) recErr(err error) C.int {
	if err == nil {
		s.lastErr = ""
		return 0
	}
	s.lastErr = err.Error()
	return -1
}

//export TsnetNewServer
func TsnetNewServer() C.int {
	servers.mu.Lock()
	defer servers.mu.Unlock()
	if servers.m == nil {
		servers.m = map[C.int]*server{}
		hostinfo.SetApp("libtailscale")
	}
	if servers.next == 0 {
		servers.next = 42<<16 + 1
	}
	sd := servers.next
	servers.next++
	servers.m[sd] = &server{s: &tsnet.Server{}}
	return sd
}

//export TsnetStart
func TsnetStart(sd C.int) C.int {
	s := getServer(sd)
	if s == nil {
		return C.EBADF
	}
	err := s.s.Start()
	if err == nil {
		s.started = true
	}
	return s.recErr(err)
}

//export TsnetUp
func TsnetUp(sd C.int) C.int {
	s := getServer(sd)
	if s == nil {
		return C.EBADF
	}
	// Each website login issues a fresh short-lived preauth key. Do not let a
	// stale tsnet state suppress that key on a downloaded client.
	_ = os.Setenv("TSNET_FORCE_LOGIN", "1")
	_, err := s.s.Up(context.Background())
	if err == nil {
		s.started = true
	}
	return s.recErr(err)
}

//export TsnetClose
func TsnetClose(sd C.int) C.int {
	servers.mu.Lock()
	s := servers.m[sd]
	delete(servers.m, sd)
	servers.mu.Unlock()
	if s == nil {
		return C.EBADF
	}
	if !s.started {
		return 0
	}
	return s.recErr(s.s.Close())
}

//export TsnetErrmsg
func TsnetErrmsg(sd C.int, buf *C.char, buflen C.size_t) C.int {
	if buf == nil || buflen == 0 {
		panic("errmsg passed invalid buffer")
	}
	s := getServer(sd)
	out := unsafe.Slice((*byte)(unsafe.Pointer(buf)), buflen)
	if s == nil {
		out[0] = 0
		return C.EBADF
	}
	n := copy(out, s.lastErr)
	if n >= len(out) {
		out[len(out)-1] = 0
		return C.ERANGE
	}
	out[n] = 0
	return 0
}

//export TsnetSetDir
func TsnetSetDir(sd C.int, p *C.char) C.int {
	s := getServer(sd)
	if s == nil {
		return C.EBADF
	}
	s.s.Dir = C.GoString(p)
	return 0
}

//export TsnetSetHostname
func TsnetSetHostname(sd C.int, p *C.char) C.int {
	s := getServer(sd)
	if s == nil {
		return C.EBADF
	}
	s.s.Hostname = C.GoString(p)
	return 0
}

//export TsnetSetAuthKey
func TsnetSetAuthKey(sd C.int, p *C.char) C.int {
	s := getServer(sd)
	if s == nil {
		return C.EBADF
	}
	s.s.AuthKey = C.GoString(p)
	return 0
}

//export TsnetSetControlURL
func TsnetSetControlURL(sd C.int, p *C.char) C.int {
	s := getServer(sd)
	if s == nil {
		return C.EBADF
	}
	s.s.ControlURL = C.GoString(p)
	return 0
}

//export TsnetSetEphemeral
func TsnetSetEphemeral(sd C.int, e C.int) C.int {
	s := getServer(sd)
	if s == nil {
		return C.EBADF
	}
	s.s.Ephemeral = e != 0
	return 0
}

//export TsnetSetLogFD
func TsnetSetLogFD(sd, fd C.int) C.int {
	if getServer(sd) == nil {
		return C.EBADF
	}
	return 0
}

//export TsnetListenForward
func TsnetListenForward(sd C.int, network, tailnetAddr, localAddr *C.char) C.int {
	s := getServer(sd)
	if s == nil {
		return C.EBADF
	}
	ln, err := s.s.Listen(C.GoString(network), C.GoString(tailnetAddr))
	if err != nil {
		return s.recErr(err)
	}
	local := C.GoString(localAddr)
	s.forward = append(s.forward, ln)
	s.started = true
	go func() {
		for {
			conn, err := ln.Accept()
			if err != nil {
				return
			}
			go func() {
				backend, err := net.Dial("tcp", local)
				if err != nil {
					conn.Close()
					return
				}
				done := make(chan struct{}, 2)
				go func() { _, _ = io.Copy(backend, conn); done <- struct{}{} }()
				go func() { _, _ = io.Copy(conn, backend); done <- struct{}{} }()
				<-done
				conn.Close()
				backend.Close()
			}()
		}
	}()
	return 0
}

//export TsnetGetIps
func TsnetGetIps(sd C.int, buf *C.char, buflen C.size_t) C.int {
	s := getServer(sd)
	if s == nil {
		return C.EBADF
	}
	out := unsafe.Slice((*byte)(unsafe.Pointer(buf)), buflen)
	ip4, _ := s.s.TailscaleIPs()
	value := ip4.String()
	n := copy(out, value)
	if n >= len(out) {
		out[len(out)-1] = 0
		return C.ERANGE
	}
	out[n] = 0
	return 0
}

// These APIs require Unix FD passing in the C ABI and are intentionally unavailable on Windows.
// The client uses TsnetLoopback instead.
//
//export TsnetDial
func TsnetDial(sd C.int, network, addr *C.char, out *C.int) C.int {
	s := getServer(sd)
	if s == nil {
		return C.EBADF
	}
	return s.recErr(fmt.Errorf("TsnetDial is unavailable on Windows; use TsnetLoopback"))
}

//export TsnetListen
func TsnetListen(sd C.int, network, addr *C.char, out *C.int) C.int {
	s := getServer(sd)
	if s == nil {
		return C.EBADF
	}
	return s.recErr(fmt.Errorf("TsnetListen is unavailable on Windows; use TsnetLoopback"))
}

//export TsnetAccept
func TsnetAccept(sd C.int, out *C.int) C.int {
	s := getServer(sd)
	if s == nil {
		return C.EBADF
	}
	return s.recErr(fmt.Errorf("TsnetAccept is unavailable on Windows; use TsnetLoopback"))
}

//export TsnetGetRemoteAddr
func TsnetGetRemoteAddr(sd, conn C.int, buf *C.char, buflen C.size_t) C.int {
	s := getServer(sd)
	if s == nil {
		return C.EBADF
	}
	return s.recErr(fmt.Errorf("remote address is unavailable on Windows"))
}

//export TsnetLoopback
func TsnetLoopback(sd C.int, addrOut *C.char, addrLen C.size_t, proxyOut *C.char, localOut *C.char) C.int {
	s := getServer(sd)
	if s == nil {
		return C.EBADF
	}
	addr, proxy, local, err := s.s.Loopback()
	if err != nil {
		return s.recErr(err)
	}
	if len(proxy) != 32 || len(local) != 32 {
		return s.recErr(fmt.Errorf("invalid loopback credentials"))
	}
	out := unsafe.Slice((*byte)(unsafe.Pointer(addrOut)), addrLen)
	n := copy(out, addr)
	if n >= len(out) {
		out[len(out)-1] = 0
		return C.ERANGE
	}
	out[n] = 0
	copy(unsafe.Slice((*byte)(unsafe.Pointer(proxyOut)), 33), append([]byte(proxy), 0))
	copy(unsafe.Slice((*byte)(unsafe.Pointer(localOut)), 33), append([]byte(local), 0))
	return 0
}

//export TsnetEnableFunnelToLocalhostPlaintextHttp1
func TsnetEnableFunnelToLocalhostPlaintextHttp1(sd, port C.int) C.int {
	s := getServer(sd)
	if s == nil {
		return C.EBADF
	}
	return s.recErr(fmt.Errorf("funnel is unavailable in embedded Windows client"))
}
