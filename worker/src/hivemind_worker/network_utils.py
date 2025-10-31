"""Network and location detection helpers for WorkerNode.

Functions expect a `node` instance providing `_log` for logging.
"""
from __future__ import annotations

from socket import socket, SOCK_DGRAM
from typing import Optional

from netifaces import interfaces, ifaddresses, AF_INET
from requests import get, exceptions


def auto_detect_location(node) -> str:
    try:
        apis = [
            'http://ip-api.com/json/',
            'https://ipapi.co/json/',
            'http://www.geoplugin.net/json.gp',
        ]
        for api_url in apis:
            try:
                response = get(api_url, timeout=5)
                if response.status_code != 200:
                    continue
                data = response.json()
                continent = None
                country = None
                if 'continent' in data:
                    continent = data.get('continent', '')
                    country = data.get('country', '')
                elif 'continent_code' in data:
                    continent_codes = {
                        'AS': 'Asia', 'AF': 'Africa', 'NA': 'North America',
                        'SA': 'South America', 'EU': 'Europe', 'OC': 'Oceania',
                    }
                    continent = continent_codes.get(data.get('continent_code', ''))
                    country = data.get('country_name', '')
                elif 'geoplugin_continentName' in data:
                    continent = data.get('geoplugin_continentName', '')
                    country = data.get('geoplugin_countryName', '')
                if continent and country:
                    mapping = {
                        'Asia': 'Asia', 'Africa': 'Africa', 'North America': 'North America',
                        'South America': 'South America', 'Europe': 'Europe', 'Oceania': 'Oceania',
                    }
                    detected = mapping.get(continent)
                    if detected:
                        node._log(f"Auto-detected location: {country} -> {detected}")
                        return detected
            except (exceptions.RequestException, Exception):
                continue
        node._log("Location detection failed, using Unknown")
        return "Unknown"
    except Exception as e:
        node._log(f"Location detection error: {e}")
        return "Unknown"


essential_vpn_prefix = '10.0.0.'

def get_local_ip(node) -> str:
    try:
        interfaces_list = interfaces()
        node._log(f"Detected network interfaces: {interfaces_list}")
        # Prefer WireGuard-like interfaces
        wg_interfaces = [iface for iface in interfaces_list if 'wg' in iface.lower() or 'wireguard' in iface.lower()]
        if wg_interfaces:
            for wg_iface in wg_interfaces:
                try:
                    addrs = ifaddresses(wg_iface)
                    if AF_INET in addrs:
                        wg_ip = addrs[AF_INET][0]['addr']
                        node._log(f"Detected WireGuard interface {wg_iface}, IP: {wg_ip}")
                        return wg_ip
                except Exception as e:
                    node._log(f"Failed to check interface {wg_iface}: {e}")
                    continue
        # VPN subnet 10.0.0.x
        for iface in interfaces_list:
            try:
                addrs = ifaddresses(iface)
                if AF_INET in addrs:
                    for addr_info in addrs[AF_INET]:
                        ip = addr_info['addr']
                        if ip.startswith(essential_vpn_prefix) and ip != f'{essential_vpn_prefix}1':
                            node._log(f"Detected VPN subnet IP: {ip} (interface: {iface})")
                            return ip
            except Exception:
                continue
    except Exception as e:
        node._log(f"Network interface detection failed: {e}")
    # Fallback: outbound socket trick
    try:
        s = socket(AF_INET, SOCK_DGRAM)
        s.connect(("8.8.8.8", 80))
        ip = s.getsockname()[0]
        s.close()
        node._log(f"Obtained IP using default method: {ip}")
        return ip
    except Exception:
        node._log("All methods failed, using 127.0.0.1")
        return "127.0.0.1"
