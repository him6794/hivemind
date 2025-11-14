import os
import ctypes
import logging


class WireGuardDLL:
    """ctypes wrapper for vpn/wireguardlib.dll on Windows.

    It expects wireguardlib.dll (and wintun.dll) to be available under dll_dir.
    """

    def __init__(self, dll_dir: str | None = None):
        if os.name != 'nt':
            raise OSError("WireGuardDLL is only supported on Windows")

        self.dll_dir = dll_dir or os.environ.get('VPN_DLL_DIR') or os.path.abspath(
            os.path.join(os.path.dirname(__file__), '..', '..', 'vpn')
        )

        try:
            # Ensure Windows can find dependent DLLs
            if hasattr(os, 'add_dll_directory'):
                os.add_dll_directory(self.dll_dir)
        except Exception as e:
            logging.debug(f"add_dll_directory failed: {e}")

        wireguard_dll_path = os.path.join(self.dll_dir, "wireguardlib.dll")
        if not os.path.exists(wireguard_dll_path):
            raise FileNotFoundError(f"wireguardlib.dll not found at {wireguard_dll_path}")

        self._dll = ctypes.CDLL(wireguard_dll_path)
        # function signatures
        self._dll.StartWireGuard.argtypes = [ctypes.c_char_p]
        self._dll.StartWireGuard.restype = ctypes.c_char_p
        self._dll.StopWireGuard.argtypes = [ctypes.c_char_p]
        self._dll.StopWireGuard.restype = ctypes.c_char_p
        self._dll.GetStatus.argtypes = [ctypes.c_char_p]
        self._dll.GetStatus.restype = ctypes.c_char_p

    def start(self, config_path: str) -> tuple[bool, str]:
        res = self._dll.StartWireGuard(config_path.encode('utf-8'))
        msg = res.decode('utf-8') if res else ''
        return msg.startswith('SUCCESS:'), msg

    def stop(self, config_path: str) -> tuple[bool, str]:
        res = self._dll.StopWireGuard(config_path.encode('utf-8'))
        msg = res.decode('utf-8') if res else ''
        return msg.startswith('SUCCESS:'), msg

    def status(self, config_path: str) -> str:
        res = self._dll.GetStatus(config_path.encode('utf-8'))
        return res.decode('utf-8') if res else 'UNKNOWN'


def default_config_path() -> str:
    # Default to ProgramData\HivemindWorker\wg0.conf
    program_data = os.environ.get('ProgramData', r'C:\ProgramData')
    base = os.path.join(program_data, 'HivemindWorker')
    os.makedirs(base, exist_ok=True)
    return os.path.join(base, 'wg0.conf')


def connect_vpn_once(dll_dir: str | None = None, cfg_path: str | None = None) -> tuple[bool, str]:
    """Start VPN if not already CONNECTED. Return (success, message)."""
    cfg = cfg_path or default_config_path()
    try:
        wg = WireGuardDLL(dll_dir)
        status = wg.status(cfg)
        if status == 'CONNECTED':
            return True, 'Already connected'
        ok, msg = wg.start(cfg)
        return ok, msg
    except Exception as e:
        logging.error(f"connect_vpn_once error: {e}")
        return False, str(e)
