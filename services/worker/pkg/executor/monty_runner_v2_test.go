package executor

import (
	"os"
	"path/filepath"
	"testing"
)

func TestNewMontyRunnerUsesConfiguredExecutable(t *testing.T) {
	tempDir := t.TempDir()
	montyPath := filepath.Join(tempDir, "monty.exe")
	if err := os.WriteFile(montyPath, []byte("test"), 0600); err != nil {
		t.Fatalf("write temp monty executable: %v", err)
	}
	t.Setenv("MONTY_EXECUTABLE", montyPath)

	runner, err := NewMontyRunner()
	if err != nil {
		t.Fatalf("NewMontyRunner failed: %v", err)
	}
	if runner.montyPath != montyPath {
		t.Fatalf("expected configured monty path %q, got %q", montyPath, runner.montyPath)
	}
}
