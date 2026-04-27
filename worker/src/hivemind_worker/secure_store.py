"""Windows DPAPI-based secure storage for small secrets (credentials, tokens).

No external dependencies; uses CryptProtectData / CryptUnprotectData via ctypes.
Data is protected to the current user by default.
"""
import os
import json
import ctypes
import ctypes.wintypes as wt
from typing import Optional, Dict, Any


class DATA_BLOB(ctypes.Structure):
    _fields_ = [("cbData", wt.DWORD), ("pbData", ctypes.POINTER(ctypes.c_char))]


_crypt32 = ctypes.windll.crypt32 if os.name == 'nt' else None
_kernel32 = ctypes.windll.kernel32 if os.name == 'nt' else None


def _bytes_to_blob(data: bytes) -> DATA_BLOB:
    blob = DATA_BLOB()
    blob.cbData = len(data)
    blob.pbData = ctypes.cast(ctypes.create_string_buffer(data), ctypes.POINTER(ctypes.c_char))
    return blob


def _blob_to_bytes(blob: DATA_BLOB) -> bytes:
    pb = ctypes.cast(blob.pbData, ctypes.c_void_p)
    size = int(blob.cbData)
    raw = ctypes.string_at(pb.value, size) if pb.value else b""
    _kernel32.LocalFree(blob.pbData)
    return raw


def protect(data: bytes, entropy: Optional[bytes] = None, machine_scope: bool = False) -> bytes:
    if os.name != 'nt':
        raise OSError('DPAPI protect requires Windows')
    in_blob = _bytes_to_blob(data)
    out_blob = DATA_BLOB()
    ent_blob = _bytes_to_blob(entropy or b"")
    flags = 0x4 if machine_scope else 0  # CRYPTPROTECT_LOCAL_MACHINE
    if not _crypt32.CryptProtectData(ctypes.byref(in_blob), None, ctypes.byref(ent_blob), None, None, flags, ctypes.byref(out_blob)):
        raise ctypes.WinError()
    return _blob_to_bytes(out_blob)


def unprotect(data: bytes, entropy: Optional[bytes] = None) -> bytes:
    if os.name != 'nt':
        raise OSError('DPAPI unprotect requires Windows')
    in_blob = _bytes_to_blob(data)
    out_blob = DATA_BLOB()
    ent_blob = _bytes_to_blob(entropy or b"")
    if not _crypt32.CryptUnprotectData(ctypes.byref(in_blob), None, ctypes.byref(ent_blob), None, None, 0, ctypes.byref(out_blob)):
        raise ctypes.WinError()
    return _blob_to_bytes(out_blob)


def _default_store_dir() -> str:
    program_data = os.environ.get('ProgramData', r'C:\ProgramData')
    base = os.path.join(program_data, 'HivemindWorker')
    os.makedirs(base, exist_ok=True)
    return base


def save_credentials(obj: Dict[str, Any], path: Optional[str] = None, entropy: Optional[bytes] = None) -> str:
    """Save a dict with user credentials (username, password, token, etc.) securely."""
    store_dir = _default_store_dir()
    path = path or os.path.join(store_dir, 'credentials.dat')
    raw = json.dumps(obj, ensure_ascii=False).encode('utf-8')
    enc = protect(raw, entropy=entropy, machine_scope=False)
    with open(path, 'wb') as f:
        f.write(enc)
    return path


def load_credentials(path: Optional[str] = None, entropy: Optional[bytes] = None) -> Optional[Dict[str, Any]]:
    store_dir = _default_store_dir()
    path = path or os.path.join(store_dir, 'credentials.dat')
    if not os.path.exists(path):
        return None
    with open(path, 'rb') as f:
        enc = f.read()
    raw = unprotect(enc, entropy=entropy)
    return json.loads(raw.decode('utf-8'))
