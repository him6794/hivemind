package bt

import (
	"archive/zip"
	"bytes"
	"crypto/sha1"
	"encoding/hex"
	"encoding/json"
	"fmt"
	"io"
	"net/url"
	"os"
	"path/filepath"
	"sort"
	"strings"
	"time"
)

type TorrentMeta struct {
	InfoHash    string    `json:"info_hash"`
	DisplayName string    `json:"display_name"`
	Length      int64     `json:"length"`
	PieceLength int       `json:"piece_length"`
	FileCount   int       `json:"file_count,omitempty"`
	Magnet      string    `json:"magnet"`
	TorrentFile string    `json:"torrent_file,omitempty"`
	Trackers    []string  `json:"trackers,omitempty"`
	CreatedAt   time.Time `json:"created_at"`
}

func CreateMagnetFromPayload(filename string, data []byte) (TorrentMeta, error) {
	meta, _, err := CreateTorrentFromPayloadWithOptions(filename, data, "", nil)
	if err != nil {
		return TorrentMeta{}, err
	}
	return meta, nil
}

func CreateTorrentFromPayload(filename string, data []byte, announce string) (TorrentMeta, []byte, error) {
	return CreateTorrentFromPayloadWithOptions(filename, data, announce, nil)
}

func CreateTorrentFromPayloadWithOptions(filename string, data []byte, announce string, announceList []string) (TorrentMeta, []byte, error) {
	if len(data) == 0 {
		return TorrentMeta{}, nil, fmt.Errorf("empty payload")
	}
	dn := strings.TrimSpace(strings.TrimSuffix(filename, filepath.Ext(filename)))
	if dn == "" {
		dn = "payload"
	}
	trackers := normalizeTrackers(announce, announceList)

	const pieceLength = 256 * 1024
	infoDict, totalLength, fileCount, err := buildInfoDict(filename, dn, data, pieceLength)
	if err != nil {
		return TorrentMeta{}, nil, err
	}
	infoBytes, err := bencode(infoDict)
	if err != nil {
		return TorrentMeta{}, nil, err
	}
	h := sha1.Sum(infoBytes)
	infoHash := hex.EncodeToString(h[:])
	q := url.Values{}
	q.Set("xt", "urn:btih:"+infoHash)
	q.Set("dn", dn)
	q.Set("xl", fmt.Sprintf("%d", totalLength))
	for _, tr := range trackers {
		q.Add("tr", tr)
	}
	magnet := "magnet:?" + q.Encode()

	top := map[string]any{
		"created by":    "hivemind-master",
		"creation date": time.Now().Unix(),
		"info":          infoDict,
	}
	if len(trackers) > 0 {
		top["announce"] = trackers[0]
		alist := make([]any, 0, len(trackers))
		for _, tr := range trackers {
			alist = append(alist, []any{tr})
		}
		top["announce-list"] = alist
	}
	torrentBytes, err := bencode(top)
	if err != nil {
		return TorrentMeta{}, nil, err
	}

	meta := TorrentMeta{
		InfoHash:    infoHash,
		DisplayName: dn,
		Length:      totalLength,
		PieceLength: pieceLength,
		FileCount:   fileCount,
		Magnet:      magnet,
		Trackers:    trackers,
		CreatedAt:   time.Now().UTC(),
	}
	return meta, torrentBytes, nil
}

func ParseMagnet(magnet string) (TorrentMeta, error) {
	u, err := url.Parse(strings.TrimSpace(magnet))
	if err != nil {
		return TorrentMeta{}, err
	}
	if u.Scheme != "magnet" {
		return TorrentMeta{}, fmt.Errorf("not a magnet uri")
	}
	xt := u.Query().Get("xt")
	if !strings.HasPrefix(strings.ToLower(xt), "urn:btih:") {
		return TorrentMeta{}, fmt.Errorf("missing xt=urn:btih")
	}
	infoHash := strings.TrimPrefix(strings.ToLower(xt), "urn:btih:")
	if len(infoHash) != 40 {
		return TorrentMeta{}, fmt.Errorf("invalid btih length")
	}
	if _, err := hex.DecodeString(infoHash); err != nil {
		return TorrentMeta{}, fmt.Errorf("invalid btih hex")
	}
	dn := u.Query().Get("dn")
	xl := int64(0)
	if s := u.Query().Get("xl"); s != "" {
		_, _ = fmt.Sscanf(s, "%d", &xl)
	}
	return TorrentMeta{
		InfoHash:    infoHash,
		DisplayName: dn,
		Length:      xl,
		PieceLength: 0,
		Magnet:      magnet,
		CreatedAt:   time.Now().UTC(),
	}, nil
}

func PersistTorrent(dir string, meta *TorrentMeta, torrentBytes []byte) error {
	if strings.TrimSpace(dir) == "" {
		return nil
	}
	if err := os.MkdirAll(dir, 0o755); err != nil {
		return err
	}
	fp := filepath.Join(dir, meta.InfoHash+".torrent")
	if err := os.WriteFile(fp, torrentBytes, 0o644); err != nil {
		return err
	}
	meta.TorrentFile = fp
	return nil
}

func PersistMeta(dir string, meta TorrentMeta) error {
	if strings.TrimSpace(dir) == "" {
		return nil
	}
	if err := os.MkdirAll(dir, 0o755); err != nil {
		return err
	}
	p := filepath.Join(dir, meta.InfoHash+".json")
	b, err := json.MarshalIndent(meta, "", "  ")
	if err != nil {
		return err
	}
	return os.WriteFile(p, b, 0o644)
}

func buildPieces(data []byte, pieceLength int) []byte {
	if pieceLength <= 0 {
		pieceLength = 256 * 1024
	}
	out := make([]byte, 0, ((len(data)+pieceLength-1)/pieceLength)*20)
	for i := 0; i < len(data); i += pieceLength {
		j := i + pieceLength
		if j > len(data) {
			j = len(data)
		}
		h := sha1.Sum(data[i:j])
		out = append(out, h[:]...)
	}
	return out
}

func bencode(v any) ([]byte, error) {
	buf := bytes.NewBuffer(nil)
	if err := bencodeTo(buf, v); err != nil {
		return nil, err
	}
	return buf.Bytes(), nil
}

func bencodeTo(buf *bytes.Buffer, v any) error {
	switch x := v.(type) {
	case string:
		buf.WriteString(fmt.Sprintf("%d:%s", len(x), x))
		return nil
	case []byte:
		buf.WriteString(fmt.Sprintf("%d:", len(x)))
		buf.Write(x)
		return nil
	case int:
		buf.WriteString(fmt.Sprintf("i%de", x))
		return nil
	case int32:
		buf.WriteString(fmt.Sprintf("i%de", x))
		return nil
	case int64:
		buf.WriteString(fmt.Sprintf("i%de", x))
		return nil
	case map[string]any:
		buf.WriteByte('d')
		keys := make([]string, 0, len(x))
		for k := range x {
			keys = append(keys, k)
		}
		sort.Strings(keys)
		for _, k := range keys {
			if err := bencodeTo(buf, k); err != nil {
				return err
			}
			if err := bencodeTo(buf, x[k]); err != nil {
				return err
			}
		}
		buf.WriteByte('e')
		return nil
	case []any:
		buf.WriteByte('l')
		for _, it := range x {
			if err := bencodeTo(buf, it); err != nil {
				return err
			}
		}
		buf.WriteByte('e')
		return nil
	default:
		return fmt.Errorf("unsupported bencode type: %T", v)
	}
}

func normalizeTrackers(announce string, announceList []string) []string {
	out := make([]string, 0)
	seen := map[string]struct{}{}
	push := func(s string) {
		s = strings.TrimSpace(s)
		if s == "" {
			return
		}
		if _, ok := seen[s]; ok {
			return
		}
		seen[s] = struct{}{}
		out = append(out, s)
	}
	push(announce)
	for _, it := range announceList {
		push(it)
	}
	return out
}

func buildInfoDict(filename, rootName string, data []byte, pieceLength int) (map[string]any, int64, int, error) {
	if !strings.HasSuffix(strings.ToLower(filename), ".zip") {
		pieces := buildPieces(data, pieceLength)
		return map[string]any{
			"name":         rootName,
			"length":       int64(len(data)),
			"piece length": pieceLength,
			"pieces":       pieces,
		}, int64(len(data)), 1, nil
	}

	zr, err := zip.NewReader(bytes.NewReader(data), int64(len(data)))
	if err != nil {
		pieces := buildPieces(data, pieceLength)
		return map[string]any{
			"name":         rootName,
			"length":       int64(len(data)),
			"piece length": pieceLength,
			"pieces":       pieces,
		}, int64(len(data)), 1, nil
	}

	files := make([]any, 0)
	concat := make([]byte, 0, len(data))
	total := int64(0)
	count := 0
	for _, f := range zr.File {
		if f.FileInfo().IsDir() {
			continue
		}
		rc, err := f.Open()
		if err != nil {
			continue
		}
		b, err := io.ReadAll(rc)
		_ = rc.Close()
		if err != nil {
			continue
		}
		concat = append(concat, b...)
		total += int64(len(b))
		count++
		files = append(files, map[string]any{
			"length": int64(len(b)),
			"path":   splitPathForTorrent(f.Name),
		})
	}

	if count == 0 {
		pieces := buildPieces(data, pieceLength)
		return map[string]any{
			"name":         rootName,
			"length":       int64(len(data)),
			"piece length": pieceLength,
			"pieces":       pieces,
		}, int64(len(data)), 1, nil
	}

	pieces := buildPieces(concat, pieceLength)
	return map[string]any{
		"name":         rootName,
		"piece length": pieceLength,
		"pieces":       pieces,
		"files":        files,
	}, total, count, nil
}

func splitPathForTorrent(p string) []any {
	p = strings.Trim(strings.ReplaceAll(p, "\\", "/"), "/")
	if p == "" {
		return []any{"file"}
	}
	parts := strings.Split(p, "/")
	out := make([]any, 0, len(parts))
	for _, it := range parts {
		if strings.TrimSpace(it) == "" {
			continue
		}
		out = append(out, it)
	}
	if len(out) == 0 {
		return []any{"file"}
	}
	return out
}
