import os
import logging
from .vpn_client import connect_vpn_once, default_config_path


def main():
    logging.basicConfig(level=logging.INFO, format='%(asctime)s - %(levelname)s - %(message)s')
    dll_dir = os.environ.get('VPN_DLL_DIR')
    cfg = os.environ.get('VPN_CONFIG_PATH') or default_config_path()
    ok, msg = connect_vpn_once(dll_dir=dll_dir, cfg_path=cfg)
    if ok:
        logging.info(f"VPN connected: {msg}")
    else:
        logging.error(f"VPN connect failed: {msg}")


if __name__ == '__main__':
    main()
