package artifact

import (
	"bytes"
	"crypto/sha256"
	"encoding/hex"
	"errors"
	"fmt"
	"sync"
)

type Chunk struct {
	ChunkID string
	SHA256  string
	Offset  int64
	Size    int64
}

type Manifest struct {
	ArtifactID    string
	Chunks        []Chunk
	Size          int64
	Compression   string
	ContentType   string
	CreatedByTask string
}

type Store struct {
	mu        sync.RWMutex
	chunkSize int
	chunks    map[string][]byte
}

type CorruptionError struct {
	ChunkID string
}

func (e CorruptionError) Error() string {
	return fmt.Sprintf("artifact chunk corruption: %s", e.ChunkID)
}

func IsCorruption(err error) bool {
	var corruption CorruptionError
	return errors.As(err, &corruption)
}

func NewMemoryStore(chunkSize int) *Store {
	if chunkSize <= 0 {
		chunkSize = 4 * 1024 * 1024
	}
	return &Store{
		chunkSize: chunkSize,
		chunks:    make(map[string][]byte),
	}
}

func (s *Store) Put(createdByTask, contentType, compression string, data []byte) (*Manifest, error) {
	if s == nil {
		return nil, errors.New("artifact store is nil")
	}
	artifactSum := sha256.Sum256(data)
	manifest := &Manifest{
		ArtifactID:    "sha256:" + hex.EncodeToString(artifactSum[:]),
		Size:          int64(len(data)),
		Compression:   compression,
		ContentType:   contentType,
		CreatedByTask: createdByTask,
	}

	s.mu.Lock()
	defer s.mu.Unlock()
	for offset := 0; offset < len(data); offset += s.chunkSize {
		end := offset + s.chunkSize
		if end > len(data) {
			end = len(data)
		}
		part := data[offset:end]
		sum := sha256.Sum256(part)
		sha := hex.EncodeToString(sum[:])
		chunkID := "sha256:" + sha
		if _, ok := s.chunks[chunkID]; !ok {
			s.chunks[chunkID] = bytes.Clone(part)
		}
		manifest.Chunks = append(manifest.Chunks, Chunk{
			ChunkID: chunkID,
			SHA256:  sha,
			Offset:  int64(offset),
			Size:    int64(len(part)),
		})
	}
	return manifest, nil
}

func (s *Store) Get(manifest *Manifest) ([]byte, error) {
	if s == nil {
		return nil, errors.New("artifact store is nil")
	}
	if manifest == nil {
		return nil, errors.New("artifact manifest is nil")
	}
	s.mu.RLock()
	defer s.mu.RUnlock()

	var out bytes.Buffer
	for _, chunk := range manifest.Chunks {
		data, ok := s.chunks[chunk.ChunkID]
		if !ok {
			return nil, fmt.Errorf("artifact chunk missing: %s", chunk.ChunkID)
		}
		sum := sha256.Sum256(data)
		if hex.EncodeToString(sum[:]) != chunk.SHA256 {
			return nil, CorruptionError{ChunkID: chunk.ChunkID}
		}
		_, _ = out.Write(data)
	}
	if int64(out.Len()) != manifest.Size {
		return nil, fmt.Errorf("artifact size mismatch: got %d want %d", out.Len(), manifest.Size)
	}
	return out.Bytes(), nil
}

func (s *Store) MissingChunks(manifest *Manifest) []Chunk {
	if s == nil || manifest == nil {
		return nil
	}
	s.mu.RLock()
	defer s.mu.RUnlock()

	var missing []Chunk
	for _, chunk := range manifest.Chunks {
		if _, ok := s.chunks[chunk.ChunkID]; !ok {
			missing = append(missing, chunk)
		}
	}
	return missing
}

func (s *Store) StoredChunkCount() int {
	if s == nil {
		return 0
	}
	s.mu.RLock()
	defer s.mu.RUnlock()
	return len(s.chunks)
}

func (s *Store) CorruptChunkForTest(chunkID string, data []byte) {
	s.mu.Lock()
	defer s.mu.Unlock()
	s.chunks[chunkID] = bytes.Clone(data)
}

func (s *Store) DeleteChunkForTest(chunkID string) {
	s.mu.Lock()
	defer s.mu.Unlock()
	delete(s.chunks, chunkID)
}
