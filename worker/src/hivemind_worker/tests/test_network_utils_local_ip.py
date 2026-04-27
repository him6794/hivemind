from __future__ import annotations

import sys
from pathlib import Path


def _import_network_utils():
    this_file = Path(__file__).resolve()
    worker_src = this_file.parents[2]
    if str(worker_src) not in sys.path:
        sys.path.insert(0, str(worker_src))
    from hivemind_worker import network_utils  # local import

    return network_utils


class _NodeStub:
    def __init__(self):
        self.logs: list[str] = []

    def _log(self, msg: str, *_args, **_kwargs):
        self.logs.append(str(msg))


def test_get_local_ip_prefers_10_subnet(monkeypatch):
    network_utils = _import_network_utils()
    node = _NodeStub()

    monkeypatch.setattr(network_utils, "interfaces", lambda: ["Ethernet0", "Wi-Fi"])

    def fake_ifaddresses(iface: str):
        if iface == "Ethernet0":
            return {network_utils.AF_INET: [{"addr": "192.168.1.10"}]}
        if iface == "Wi-Fi":
            return {network_utils.AF_INET: [{"addr": "10.0.0.23"}]}
        return {}

    monkeypatch.setattr(network_utils, "ifaddresses", fake_ifaddresses)

    ip = network_utils.get_local_ip(node)
    assert ip == "10.0.0.23"


def test_get_local_ip_falls_back_to_wireguard_if_no_10(monkeypatch):
    network_utils = _import_network_utils()
    node = _NodeStub()

    monkeypatch.setattr(network_utils, "interfaces", lambda: ["Ethernet0", "WireGuard"])

    def fake_ifaddresses(iface: str):
        if iface == "WireGuard":
            return {network_utils.AF_INET: [{"addr": "172.16.0.5"}]}
        if iface == "Ethernet0":
            return {network_utils.AF_INET: [{"addr": "192.168.1.10"}]}
        return {}

    monkeypatch.setattr(network_utils, "ifaddresses", fake_ifaddresses)

    ip = network_utils.get_local_ip(node)
    assert ip == "172.16.0.5"
