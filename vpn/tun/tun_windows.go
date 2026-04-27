//go:build windows

/* SPDX-License-Identifier: MIT
 *
 * Copyright (C) 2017-2025 WireGuard LLC. All Rights Reserved.
 */

package tun

import (
	"errors"
	"fmt"
	"os"

	"golang.org/x/sys/windows"
	"golang.zx2c4.com/wintun"
	"golang.zx2c4.com/wireguard/rwcancel"
)

const (
	// Wintun interface pool name
	poolName = "WireGuard"
)

type NativeTun struct {
	wintun   *wintun.Adapter
	events   chan Event
	errors   chan error
	close    chan struct{}
	rwcancel *rwcancel.RWCancel
	session  wintun.Session
	readWait *windows.Overlapped
	readBuf  []byte
}

func CreateTUN(ifname string, mtu int) (Device, error) {
	adapter, err := wintun.CreateAdapter(poolName, ifname, nil)
	if err != nil {
		return nil, fmt.Errorf("create adapter: %w", err)
	}

	session, err := adapter.StartSession(wintun.ReceiveWait)
	if err != nil {
		adapter.Close()
		return nil, fmt.Errorf("start session: %w", err)
	}

	tun := &NativeTun{
		wintun:   adapter,
		events:   make(chan Event, 10),
		errors:   make(chan error, 1),
		close:    make(chan struct{}),
		rwcancel: rwcancel.NewRWCancel(windows.AF_INET),
		session:  session,
		readWait: &windows.Overlapped{},
		readBuf:  make([]byte, mtu+80), // MTU + header
	}

	go tun.routine()

	return tun, nil
}

func CreateTUNFromFile(file *os.File, mtu int) (Device, error) {
	return nil, errors.New("CreateTUNFromFile not supported on Windows")
}

func (tun *NativeTun) Name() (string, error) {
	return tun.wintun.Name(), nil
}

func (tun *NativeTun) File() *os.File {
	return nil
}

func (tun *NativeTun) Events() <-chan Event {
	return tun.events
}

func (tun *NativeTun) Read(buf []byte, offset int) (int, error) {
	select {
	case <-tun.close:
		return 0, os.ErrClosed
	default:
	}

	data, err := tun.session.ReceivePacket()
	if err != nil {
		return 0, err
	}
	copy(buf[offset:], data)
	size := len(data)
	data.Release()
	return size, nil
}

func (tun *NativeTun) Write(buf []byte, offset int) (int, error) {
	packet, err := tun.session.AllocateSendPacket(len(buf) - offset)
	if err != nil {
		return 0, err
	}
	copy(packet, buf[offset:])
	tun.session.SendPacket(packet)
	return len(buf) - offset, nil
}

func (tun *NativeTun) Flush() error {
	return nil
}

func (tun *NativeTun) Close() error {
	close(tun.close)
	tun.rwcancel.Cancel()
	tun.session.Close()
	tun.wintun.Close()
	return nil
}

func (tun *NativeTun) MTU() (int, error) {
	return tun.wintun.MTU(), nil
}

func (tun *NativeTun) BatchSize() int {
	return 1
}

func (tun *NativeTun) routine() {
	for {
		select {
		case <-tun.close:
			return
		case <-tun.rwcancel.C:
			return
		}
	}
}
