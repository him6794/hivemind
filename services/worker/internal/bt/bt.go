package bt

import (
	"crypto/sha1"
	"encoding/hex"
	"fmt"
	"net/url"
	"strings"
)

type Magnet struct {
	InfoHash    string
	DisplayName string
	Raw         string
}

func ParseMagnet(raw string) (Magnet, error) {
	u, err := url.Parse(strings.TrimSpace(raw))
	if err != nil {
		return Magnet{}, err
	}
	if u.Scheme != "magnet" {
		return Magnet{}, fmt.Errorf("not a magnet uri")
	}
	xt := strings.ToLower(u.Query().Get("xt"))
	if !strings.HasPrefix(xt, "urn:btih:") {
		return Magnet{}, fmt.Errorf("missing xt=urn:btih")
	}
	infoHash := strings.TrimPrefix(xt, "urn:btih:")
	if len(infoHash) != 40 {
		return Magnet{}, fmt.Errorf("invalid btih length")
	}
	if _, err := hex.DecodeString(infoHash); err != nil {
		return Magnet{}, fmt.Errorf("invalid btih hex")
	}
	return Magnet{InfoHash: infoHash, DisplayName: u.Query().Get("dn"), Raw: raw}, nil
}

// ParseTorrentInfoHash computes info-hash from raw .torrent bytes.
func ParseTorrentInfoHash(torrent []byte) (string, error) {
	if len(torrent) == 0 {
		return "", fmt.Errorf("empty torrent data")
	}
	if torrent[0] != 'd' {
		return "", fmt.Errorf("torrent root is not dictionary")
	}

	i := 1
	for i < len(torrent) {
		if torrent[i] == 'e' {
			break
		}
		key, next, err := parseBString(torrent, i)
		if err != nil {
			return "", err
		}
		i = next
		if key == "info" {
			end, err := parseBValueEnd(torrent, i)
			if err != nil {
				return "", err
			}
			h := sha1.Sum(torrent[i:end])
			return hex.EncodeToString(h[:]), nil
		}
		end, err := parseBValueEnd(torrent, i)
		if err != nil {
			return "", err
		}
		i = end
	}
	return "", fmt.Errorf("info dictionary not found")
}

func parseBString(b []byte, i int) (string, int, error) {
	if i >= len(b) {
		return "", i, fmt.Errorf("unexpected eof")
	}
	j := i
	for j < len(b) && b[j] >= '0' && b[j] <= '9' {
		j++
	}
	if j == i || j >= len(b) || b[j] != ':' {
		return "", i, fmt.Errorf("invalid bencode string")
	}
	var n int
	_, err := fmt.Sscanf(string(b[i:j]), "%d", &n)
	if err != nil || n < 0 {
		return "", i, fmt.Errorf("invalid bencode string length")
	}
	start := j + 1
	end := start + n
	if end > len(b) {
		return "", i, fmt.Errorf("bencode string overflow")
	}
	return string(b[start:end]), end, nil
}

func parseBValueEnd(b []byte, i int) (int, error) {
	if i >= len(b) {
		return i, fmt.Errorf("unexpected eof")
	}
	switch b[i] {
	case 'i':
		j := i + 1
		for j < len(b) && b[j] != 'e' {
			j++
		}
		if j >= len(b) {
			return i, fmt.Errorf("unterminated integer")
		}
		return j + 1, nil
	case 'l':
		j := i + 1
		for j < len(b) && b[j] != 'e' {
			end, err := parseBValueEnd(b, j)
			if err != nil {
				return i, err
			}
			j = end
		}
		if j >= len(b) {
			return i, fmt.Errorf("unterminated list")
		}
		return j + 1, nil
	case 'd':
		j := i + 1
		for j < len(b) && b[j] != 'e' {
			_, next, err := parseBString(b, j)
			if err != nil {
				return i, err
			}
			j = next
			end, err := parseBValueEnd(b, j)
			if err != nil {
				return i, err
			}
			j = end
		}
		if j >= len(b) {
			return i, fmt.Errorf("unterminated dictionary")
		}
		return j + 1, nil
	default:
		_, end, err := parseBString(b, i)
		if err != nil {
			return i, err
		}
		return end, nil
	}
}
