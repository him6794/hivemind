package main

import (
	"testing"
	"time"
)

func TestNodepoolCallTimeoutFromEnv(t *testing.T) {
	t.Run("configured seconds", func(t *testing.T) {
		t.Setenv("MASTER_NODEPOOL_TIMEOUT_SEC", "17")
		if got := nodepoolCallTimeout(); got != 17*time.Second {
			t.Fatalf("nodepoolCallTimeout()=%s, want 17s", got)
		}
	})

	t.Run("fallback is latency tolerant", func(t *testing.T) {
		t.Setenv("MASTER_NODEPOOL_TIMEOUT_SEC", "0")
		if got := nodepoolCallTimeout(); got < 15*time.Second {
			t.Fatalf("nodepoolCallTimeout()=%s, want at least 15s", got)
		}
	})
}
