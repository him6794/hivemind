"""Auto-update module for Hivemind Worker.

Periodically checks a remote manifest (Cloudflare Worker endpoint) for the latest
version and downloads a new executable/build if a higher version is available.

Manifest JSON example:
{
  "version": "1.2.3",
  "download_url": "https://example-cdn/hivemind-worker-1.2.3.exe",
  "signature": "<optional hex/base64 signature>"
}

Signature verification is left as TODO (can be implemented with public key).
"""
from __future__ import annotations
import os
import time
import json
import logging
import threading
import hashlib
import requests
from typing import Optional, Dict

MANIFEST_URL = os.environ.get('WORKER_UPDATE_MANIFEST', 'https://example.com/hivemind/worker/manifest.json')
CHECK_INTERVAL_SEC = int(os.environ.get('WORKER_UPDATE_INTERVAL_SEC', '1800'))  # default 30 min
DOWNLOAD_DIR = os.environ.get('WORKER_DOWNLOAD_DIR') or os.path.join(os.environ.get('ProgramData', r'C:\ProgramData'), 'HivemindWorker', 'updates')
CURRENT_VERSION_FILE = os.path.join(DOWNLOAD_DIR, 'current_version.txt')


def _ensure_dirs():
    os.makedirs(DOWNLOAD_DIR, exist_ok=True)


def _read_current_version() -> str:
    if not os.path.exists(CURRENT_VERSION_FILE):
        return "0.0.0"
    try:
        with open(CURRENT_VERSION_FILE, 'r', encoding='utf-8') as f:
            return f.read().strip() or "0.0.0"
    except Exception:
        return "0.0.0"


def _write_current_version(version: str):
    try:
        with open(CURRENT_VERSION_FILE, 'w', encoding='utf-8') as f:
            f.write(version)
    except Exception as e:
        logging.error(f"Failed to write current version: {e}")


def _version_tuple(v: str):
    try:
        return tuple(int(x) for x in v.split('.'))
    except Exception:
        return (0, 0, 0)


def fetch_manifest(url: str = MANIFEST_URL) -> Optional[Dict]:
    try:
        resp = requests.get(url, timeout=10)
        if resp.status_code != 200:
            logging.warning(f"Manifest request failed: {resp.status_code}")
            return None
        return resp.json()
    except Exception as e:
        logging.warning(f"Manifest fetch error: {e}")
        return None


def download_file(url: str, dest_path: str) -> bool:
    try:
        r = requests.get(url, timeout=60, stream=True)
        if r.status_code != 200:
            logging.error(f"Download failed with status {r.status_code}")
            return False
        tmp_path = dest_path + '.part'
        with open(tmp_path, 'wb') as f:
            for chunk in r.iter_content(chunk_size=65536):
                if chunk:
                    f.write(chunk)
        os.replace(tmp_path, dest_path)
        return True
    except Exception as e:
        logging.error(f"Download error: {e}")
        return False


def compute_sha256(path: str) -> str:
    h = hashlib.sha256()
    with open(path, 'rb') as f:
        for block in iter(lambda: f.read(65536), b''):
            h.update(block)
    return h.hexdigest()


def perform_update(manifest: Dict) -> Optional[str]:
    version = manifest.get('version')
    url = manifest.get('download_url')
    if not version or not url:
        logging.warning('Manifest missing version or download_url')
        return None
    target_filename = f"hivemind-worker-{version}.exe"
    target_path = os.path.join(DOWNLOAD_DIR, target_filename)
    if os.path.exists(target_path):
        logging.info(f"Version {version} already downloaded")
        return target_path
    logging.info(f"Downloading new worker version {version} from {url}")
    if not download_file(url, target_path):
        return None
    # Optional signature / hash verification
    manifest_hash = manifest.get('sha256')
    if manifest_hash:
        local_hash = compute_sha256(target_path)
        if local_hash.lower() != manifest_hash.lower():
            logging.error(f"Hash mismatch: expected {manifest_hash}, got {local_hash}")
            try:
                os.remove(target_path)
            except Exception:
                pass
            return None
    _write_current_version(version)
    logging.info(f"Update downloaded: {target_path}")
    return target_path


def update_loop(stop_event: threading.Event):
    _ensure_dirs()
    while not stop_event.is_set():
        try:
            manifest = fetch_manifest()
            if manifest:
                new_version = manifest.get('version', '0.0.0')
                current_version = _read_current_version()
                if _version_tuple(new_version) > _version_tuple(current_version):
                    logging.info(f"Found new version {new_version} > {current_version}; updating...")
                    perform_update(manifest)
                else:
                    logging.debug(f"No update needed (current {current_version}, remote {new_version})")
        except Exception as e:
            logging.error(f"Update loop error: {e}")
        stop_event.wait(CHECK_INTERVAL_SEC)


def start_update_thread(stop_event: threading.Event) -> threading.Thread:
    t = threading.Thread(target=update_loop, name='AutoUpdate', args=(stop_event,), daemon=True)
    t.start()
    return t
