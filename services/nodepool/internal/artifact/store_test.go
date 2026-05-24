package artifact

import (
	"bytes"
	"crypto/sha256"
	"encoding/hex"
	"testing"
)

func TestStoreBuildsManifestWithChunksAndDeduplicatesContent(t *testing.T) {
	store := NewMemoryStore(4)
	data := []byte("aaaabbbbcccc")

	first, err := store.Put("task-1", "application/octet-stream", "zstd", data)
	if err != nil {
		t.Fatalf("first put: %v", err)
	}
	second, err := store.Put("task-1", "application/octet-stream", "zstd", data)
	if err != nil {
		t.Fatalf("second put: %v", err)
	}

	if first.ArtifactID != second.ArtifactID {
		t.Fatalf("artifact ids differ for identical content: %s != %s", first.ArtifactID, second.ArtifactID)
	}
	if store.StoredChunkCount() != 3 {
		t.Fatalf("stored chunk count=%d, want 3", store.StoredChunkCount())
	}
	if len(first.Chunks) != 3 {
		t.Fatalf("manifest chunk count=%d, want 3", len(first.Chunks))
	}
	if first.Size != int64(len(data)) {
		t.Fatalf("manifest size=%d, want %d", first.Size, len(data))
	}
	if first.Compression != "zstd" {
		t.Fatalf("compression=%q, want zstd", first.Compression)
	}
}

func TestStoreDetectsChunkCorruption(t *testing.T) {
	store := NewMemoryStore(4)
	manifest, err := store.Put("task-1", "text/plain", "none", []byte("aaaabbbb"))
	if err != nil {
		t.Fatalf("put: %v", err)
	}

	store.CorruptChunkForTest(manifest.Chunks[0].ChunkID, []byte("zzzz"))

	_, err = store.Get(manifest)
	if err == nil {
		t.Fatal("expected corruption error")
	}
	if !IsCorruption(err) {
		t.Fatalf("expected corruption error, got %v", err)
	}
}

func TestStoreReportsMissingChunksForResume(t *testing.T) {
	store := NewMemoryStore(4)
	manifest, err := store.Put("task-1", "text/plain", "none", []byte("aaaabbbbcccc"))
	if err != nil {
		t.Fatalf("put: %v", err)
	}

	store.DeleteChunkForTest(manifest.Chunks[1].ChunkID)
	missing := store.MissingChunks(manifest)
	if len(missing) != 1 {
		t.Fatalf("missing chunks=%d, want 1", len(missing))
	}
	if missing[0].ChunkID != manifest.Chunks[1].ChunkID {
		t.Fatalf("missing chunk=%s, want %s", missing[0].ChunkID, manifest.Chunks[1].ChunkID)
	}
}

func TestStoreReassemblesVerifiedContent(t *testing.T) {
	store := NewMemoryStore(3)
	data := []byte("abcdefghi")
	manifest, err := store.Put("task-2", "text/plain", "none", data)
	if err != nil {
		t.Fatalf("put: %v", err)
	}

	got, err := store.Get(manifest)
	if err != nil {
		t.Fatalf("get: %v", err)
	}
	if !bytes.Equal(got, data) {
		t.Fatalf("reassembled content=%q, want %q", got, data)
	}

	sum := sha256.Sum256(data)
	if manifest.ArtifactID != "sha256:"+hex.EncodeToString(sum[:]) {
		t.Fatalf("artifact id=%q, want sha256 content id", manifest.ArtifactID)
	}
}
