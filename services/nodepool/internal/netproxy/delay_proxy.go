package netproxy

import (
	"fmt"
	"io"
	"math/rand"
	"net"
	"sync"
	"sync/atomic"
	"time"
)

type DelayProxyConfig struct {
	Name       string
	ListenHost string
	ListenPort int
	TargetHost string
	TargetPort int
	Latency    time.Duration
	Jitter     time.Duration
}

type DelayProxy struct {
	cfg      DelayProxyConfig
	listener net.Listener
	stopOnce sync.Once
	stop     chan struct{}
	wg       sync.WaitGroup

	connections    int64
	bytesForwarded int64
}

func NewDelayProxy(cfg DelayProxyConfig) *DelayProxy {
	if cfg.ListenHost == "" {
		cfg.ListenHost = "127.0.0.1"
	}
	if cfg.TargetHost == "" {
		cfg.TargetHost = "127.0.0.1"
	}
	return &DelayProxy{
		cfg:  cfg,
		stop: make(chan struct{}),
	}
}

func (p *DelayProxy) Start() error {
	if p == nil {
		return fmt.Errorf("delay proxy is nil")
	}
	ln, err := net.Listen("tcp", p.ListenAddr())
	if err != nil {
		return err
	}
	p.listener = ln
	p.wg.Add(1)
	go p.acceptLoop()
	return nil
}

func (p *DelayProxy) Stop() {
	if p == nil {
		return
	}
	p.stopOnce.Do(func() {
		close(p.stop)
		if p.listener != nil {
			_ = p.listener.Close()
		}
	})
	done := make(chan struct{})
	go func() {
		p.wg.Wait()
		close(done)
	}()
	select {
	case <-done:
	case <-time.After(2 * time.Second):
	}
}

func (p *DelayProxy) ListenAddr() string {
	return fmt.Sprintf("%s:%d", p.cfg.ListenHost, p.cfg.ListenPort)
}

func (p *DelayProxy) TargetAddr() string {
	return fmt.Sprintf("%s:%d", p.cfg.TargetHost, p.cfg.TargetPort)
}

func (p *DelayProxy) Connections() int64 {
	return atomic.LoadInt64(&p.connections)
}

func (p *DelayProxy) BytesForwarded() int64 {
	return atomic.LoadInt64(&p.bytesForwarded)
}

func (p *DelayProxy) acceptLoop() {
	defer p.wg.Done()
	for {
		conn, err := p.listener.Accept()
		if err != nil {
			select {
			case <-p.stop:
				return
			default:
				continue
			}
		}
		atomic.AddInt64(&p.connections, 1)
		p.wg.Add(1)
		go p.handle(conn)
	}
}

func (p *DelayProxy) handle(client net.Conn) {
	defer p.wg.Done()
	upstream, err := net.DialTimeout("tcp", p.TargetAddr(), 5*time.Second)
	if err != nil {
		_ = client.Close()
		return
	}

	var pipeWG sync.WaitGroup
	pipeWG.Add(2)
	go func() {
		defer pipeWG.Done()
		p.pipe(client, upstream)
	}()
	go func() {
		defer pipeWG.Done()
		p.pipe(upstream, client)
	}()
	pipeWG.Wait()
}

func (p *DelayProxy) pipe(src net.Conn, dst net.Conn) {
	defer src.Close()
	defer dst.Close()
	buf := make([]byte, 64*1024)
	for {
		n, err := src.Read(buf)
		if n > 0 {
			p.delay()
			if _, writeErr := dst.Write(buf[:n]); writeErr != nil {
				return
			}
			atomic.AddInt64(&p.bytesForwarded, int64(n))
		}
		if err != nil {
			if err != io.EOF {
				return
			}
			return
		}
	}
}

func (p *DelayProxy) delay() {
	delay := p.cfg.Latency
	if p.cfg.Jitter > 0 {
		delay += time.Duration(rand.Int63n(int64(p.cfg.Jitter)))
	}
	if delay > 0 {
		time.Sleep(delay)
	}
}
