package bt

import (
	"archive/zip"
	"bytes"
	"io"
	"os"
	"path/filepath"
	"testing"
)

// createZipPayload creates an in-memory zip with one file name/content
func createZipPayload(name, content string) ([]byte, error) {
	buf := bytes.NewBuffer(nil)
	zw := zip.NewWriter(buf)
	w, err := zw.Create(name)
	if err != nil {
		return nil, err
	}
	if _, err := io.WriteString(w, content); err != nil {
		return nil, err
	}
	if err := zw.Close(); err != nil {
		return nil, err
	}
	return buf.Bytes(), nil
}

func TestCreateTorrentFromPayloadAndPersist(t *testing.T) {
	data, err := createZipPayload("main.py", "print('hello')")
	if err != nil {
		t.Fatalf("create zip payload failed: %v", err)
	}
	announce := ""
	announceList := []string{}
	meta, torrentBytes, err := CreateTorrentFromPayloadWithOptions("main.zip", data, announce, announceList)
	if err != nil {
		t.Fatalf("CreateTorrentFromPayloadWithOptions failed: %v", err)
	}
	if meta.InfoHash == "" {
		t.Fatalf("empty infohash")
	}
	if len(torrentBytes) == 0 {
		t.Fatalf("empty torrent bytes")
	}

	// persist to temp dir
	d := t.TempDir()
	if err := PersistTorrent(d, &meta, torrentBytes); err != nil {
		t.Fatalf("PersistTorrent failed: %v", err)
	}
	expectedPath := filepath.Join(d, meta.InfoHash+".torrent")
	if _, err := os.Stat(expectedPath); err != nil {
		t.Fatalf("torrent file not found at %s: %v", expectedPath, err)
	}
}
