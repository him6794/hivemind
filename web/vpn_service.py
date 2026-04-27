import json
import threading
import time
from datetime import datetime
from http.server import HTTPServer, BaseHTTPRequestHandler
from socketserver import ThreadingMixIn
from urllib.parse import urlparse, parse_qs
import uuid
import os
import sys
from collections import defaultdict
from wireguard_server import WireGuardServer
import platform
from dotenv import load_dotenv

# 載入環境變數
load_dotenv()

# 從環境變數讀取配置
WIREGUARD_CONFIG = {
    'server_ip': os.getenv('WIREGUARD_SERVER_IP', 'hivemindvpn.justin0711.com'),
    'server_port': int(os.getenv('WIREGUARD_SERVER_PORT', '51820')),
    'network': os.getenv('WIREGUARD_NETWORK', '10.0.0.0/24'),
    'dns_servers': os.getenv('WIREGUARD_DNS_SERVERS', '8.8.8.8,1.1.1.1').split(','),
    'persistent_keepalive': int(os.getenv('WIREGUARD_PERSISTENT_KEEPALIVE', '25')),
    'mtu': int(os.getenv('WIREGUARD_MTU', '1420')),
    'server_public_key': os.getenv('WIREGUARD_SERVER_PUBLIC_KEY', '9ClTcJ/m1iYXo6CSBZhQhPFeARtKn4pr+sKF/5HXDWs=')
}

SECURITY_CONFIG = {
    'rate_limit_seconds': int(os.getenv('RATE_LIMIT_SECONDS', '5')),
    'max_clients_per_user': int(os.getenv('MAX_CLIENTS_PER_USER', '5'))
}

def get_wireguard_config_dir():
    return os.getenv('WIREGUARD_CONFIG_DIR', '/mnt/myusb/hivemind/web/wireguard_configs')

STORAGE_CONFIG = {
    'config_dir': get_wireguard_config_dir(),
    'log_dir': os.getenv('LOG_DIR', '/mnt/myusb/hivemind/web/logs'),
}

wireguard_server = None
ip_rate_limit = defaultdict(float)

class ThreadedHTTPServer(ThreadingMixIn, HTTPServer):
    daemon_threads = True
    allow_reuse_address = True

class VPNServiceHandler(BaseHTTPRequestHandler):
    def _set_headers(self, status_code=200, content_type='application/json'):
        self.send_response(status_code)
        self.send_header('Content-Type', content_type)
        self.send_header('Access-Control-Allow-Origin', '*')
        self.send_header('Access-Control-Allow-Methods', 'GET, POST, DELETE, OPTIONS')
        self.send_header('Access-Control-Allow-Headers', 'Content-Type, Authorization')
        self.end_headers()

    def _send_json_response(self, data, status_code=200):
        self._set_headers(status_code)
        response = json.dumps(data, ensure_ascii=False, indent=2)
        self.wfile.write(response.encode('utf-8'))

    def _get_request_data(self):
        try:
            content_length = int(self.headers.get('Content-Length', 0))
            if content_length > 0:
                post_data = self.rfile.read(content_length)
                return json.loads(post_data.decode('utf-8'))
            return {}
        except:
            return {}

    def _get_client_ip(self):
        forwarded_for = self.headers.get('X-Forwarded-For')
        if forwarded_for:
            return forwarded_for.split(',')[0].strip()
        return self.client_address[0]

    def do_OPTIONS(self):
        self._set_headers()

    def do_GET(self):
        parsed_path = urlparse(self.path)
        path = parsed_path.path

        if path == '/vpn/status':
            self._handle_get_status()
        elif path == '/vpn/config_info':
            self._handle_get_config_info()
        elif path == '/health':
            self._handle_health_check()
        else:
            self._send_json_response({'error': '找不到請求的路徑'}, 404)

    def do_POST(self):
        parsed_path = urlparse(self.path)
        path = parsed_path.path

        if path == '/vpn/create_client':
            self._handle_create_client()
        else:
            self._send_json_response({'error': '找不到請求的路徑'}, 404)

    def do_DELETE(self):
        parsed_path = urlparse(self.path)
        path = parsed_path.path

        if path.startswith('/vpn/remove_client/'):
            client_name = path.split('/')[-1]
            self._handle_remove_client(client_name)
        else:
            self._send_json_response({'error': '找不到請求的路徑'}, 404)

    def _handle_create_client(self):
        global wireguard_server

        if not wireguard_server:
            self._send_json_response({'error': 'VPN 服務不可用'}, 500)
            return

        try:
            data = self._get_request_data()
            client_ip = data.get('client_ip', self._get_client_ip())
            custom_name = data.get('client_name', '').strip()

            # 檢查限流
            if not check_rate_limit(client_ip):
                self._send_json_response({
                    'error': '請求過於頻繁，請等待 5 秒後再試',
                    'rate_limit_seconds': SECURITY_CONFIG['rate_limit_seconds']
                }, 429)
                return

            # 生成客戶端名稱
            timestamp = int(datetime.now().timestamp())
            ip_suffix = client_ip.replace('.', '_').replace(':', '_')

            if custom_name and len(custom_name) <= 20 and custom_name.replace('_', '').replace('-', '').isalnum():
                client_name = f"{custom_name}_{timestamp}"
            else:
                client_name = f"client_{ip_suffix}_{timestamp}"

            # 檢查客戶端名稱是否已存在
            if client_name in wireguard_server.clients:
                client_name = f"{client_name}_{uuid.uuid4().hex[:6]}"

            # 生成配置
            client_config = wireguard_server.add_client(client_name)

            dns_servers = ', '.join(WIREGUARD_CONFIG['dns_servers'])
            server_endpoint = f"{WIREGUARD_CONFIG['server_ip']}:{WIREGUARD_CONFIG['server_port']}"
            server_public_key = WIREGUARD_CONFIG['server_public_key']

            vpn_config_content = f"""[Interface]
PrivateKey = {client_config['private_key']}
Address = {client_config['ip']}/24
DNS = {dns_servers}
MTU = {WIREGUARD_CONFIG['mtu']}

[Peer]
PublicKey = {server_public_key}
Endpoint = {server_endpoint}
AllowedIPs = {WIREGUARD_CONFIG['network']}
PersistentKeepalive = {WIREGUARD_CONFIG['persistent_keepalive']}
"""

            config_file_path = os.path.join(STORAGE_CONFIG['config_dir'], f"{client_name}.conf")
            with open(config_file_path, 'w', encoding='utf-8') as f:
                f.write(vpn_config_content)

            print(f"生成客戶端配置: {client_name}")
            print(f"配置文件路徑: {config_file_path}")
            print(f"使用服務器公鑰: {server_public_key}")

            self._send_json_response({
                'success': True,
                'message': 'VPN 配置生成成功',
                'config': vpn_config_content,
                'client_name': client_name,
                'client_ip': client_config['ip'],
                'server_endpoint': server_endpoint,
                'generated_at': datetime.now().isoformat(),
                'config_file_path': config_file_path,
                'filename': f"{client_name}.conf"
            })

        except Exception as e:
            print(f"生成 VPN 配置失敗: {str(e)}")
            import traceback
            traceback.print_exc()
            self._send_json_response({
                'success': False,
                'error': f'生成 VPN 配置失敗: {str(e)}'
            }, 500)

    def _handle_get_status(self):
        global wireguard_server

        if not wireguard_server:
            self._send_json_response({'error': 'VPN 服務不可用'}, 500)
            return

        try:
            self._send_json_response({
                'server_status': 'running' if hasattr(wireguard_server, 'monitoring_active') and wireguard_server.monitoring_active else 'stopped',
                'server_ip': WIREGUARD_CONFIG['server_ip'],
                'server_port': WIREGUARD_CONFIG['server_port'],
                'total_clients': len(wireguard_server.clients),
                'clients': list(wireguard_server.clients.keys())
            })
        except Exception as e:
            self._send_json_response({'error': str(e)}, 500)

    def _handle_remove_client(self, client_name):
        global wireguard_server

        if not wireguard_server:
            self._send_json_response({'error': 'VPN 服務不可用'}, 500)
            return

        try:
            if wireguard_server.remove_client(client_name):
                self._send_json_response({'message': f'客戶端 {client_name} 已移除'})
            else:
                self._send_json_response({'error': '客戶端不存在'}, 404)
        except Exception as e:
            self._send_json_response({'error': str(e)}, 500)

    def _handle_get_config_info(self):
        self._send_json_response({
            'server_info': {
                'endpoint': f"{WIREGUARD_CONFIG['server_ip']}:{WIREGUARD_CONFIG['server_port']}",
                'network': WIREGUARD_CONFIG['network'],
                'dns_servers': WIREGUARD_CONFIG['dns_servers'],
                'persistent_keepalive': WIREGUARD_CONFIG['persistent_keepalive'],
                'mtu': WIREGUARD_CONFIG['mtu']
            },
            'limits': {
                'rate_limit_seconds': SECURITY_CONFIG['rate_limit_seconds'],
                'max_clients_per_user': SECURITY_CONFIG['max_clients_per_user']
            },
            'total_clients': len(wireguard_server.clients) if wireguard_server else 0
        })

    def _handle_health_check(self):
        self._send_json_response({
            'status': 'healthy' if wireguard_server else 'error',
            'timestamp': datetime.now().isoformat(),
            'wireguard_active': bool(wireguard_server),
            'total_clients': len(wireguard_server.clients) if wireguard_server else 0
        })

    def log_message(self, format, *args):
        if '40' in str(args[1]) or '50' in str(args[1]):
            print(f"[{datetime.now().strftime('%Y-%m-%d %H:%M:%S')}] {format % args}")

def generate_wireguard_keys():
    try:
        import subprocess
        private_key_result = subprocess.run(['wg', 'genkey'], capture_output=True, text=True)
        if private_key_result.returncode != 0:
            raise Exception("無法生成私鑰")
        private_key = private_key_result.stdout.strip()
        public_key_result = subprocess.run(
            ['wg', 'pubkey'],
            input=private_key,
            capture_output=True,
            text=True
        )
        if public_key_result.returncode != 0:
            raise Exception("無法生成公鑰")
        public_key = public_key_result.stdout.strip()
        return private_key, public_key
    except Exception as e:
        print(f"生成密鑰失敗: {e}")
        return None, None

def load_server_config_from_file():
    possible_config_paths = [
        '/etc/wireguard/wg0.conf',
        '/mnt/myusb/hivemind/主控端/wg0.conf',
        os.path.join(STORAGE_CONFIG['config_dir'], 'server_wg0.conf'),
        os.path.join(STORAGE_CONFIG['config_dir'], 'wg0.conf')
    ]
    for config_path in possible_config_paths:
        if os.path.exists(config_path):
            try:
                print(f"嘗試從 {config_path} 讀取服務器配置")
                with open(config_path, 'r') as f:
                    content = f.read()
                lines = content.split('\n')
                private_key = None
                for line in lines:
                    line = line.strip()
                    if line.startswith('PrivateKey ='):
                        private_key = line.split('=')[1].strip()
                        break
                if private_key:
                    public_key = generate_public_key_from_private(private_key)
                    if public_key:
                        print(f"從配置文件成功讀取服務器密鑰對")
                        return private_key, public_key
            except Exception as e:
                print(f"讀取配置文件 {config_path} 失敗: {e}")
                continue
    return None, None

def generate_public_key_from_private(private_key):
    try:
        import subprocess
        result = subprocess.run(
            ['wg', 'pubkey'],
            input=private_key,
            capture_output=True,
            text=True
        )
        if result.returncode == 0:
            return result.stdout.strip()
        else:
            print(f"生成公鑰失敗: {result.stderr}")
            return None
    except Exception as e:
        print(f"生成公鑰時發生錯誤: {e}")
        return None

def init_wireguard_server():
    global wireguard_server
    try:
        os.makedirs(STORAGE_CONFIG['config_dir'], exist_ok=True)
        os.makedirs(STORAGE_CONFIG['log_dir'], exist_ok=True)
        wireguard_server = WireGuardServer(
            interface_name="wg0",
            server_port=WIREGUARD_CONFIG['server_port'],
            network=WIREGUARD_CONFIG['network']
        )
        if hasattr(wireguard_server, 'config_dir'):
            wireguard_server.config_dir = STORAGE_CONFIG['config_dir']
        print(f"WireGuard 伺服器初始化成功")
        print(f"  - 端口: {WIREGUARD_CONFIG['server_port']}")
        print(f"  - 配置目錄: {STORAGE_CONFIG['config_dir']}")
        print(f"  - 服務器公鑰: {WIREGUARD_CONFIG['server_public_key']}")
    except Exception as e:
        print(f"WireGuard 伺服器初始化失敗: {e}")
        import traceback
        traceback.print_exc()
        wireguard_server = None

def check_rate_limit(ip_address):
    current_time = time.time()
    last_request_time = ip_rate_limit[ip_address]
    if current_time - last_request_time < SECURITY_CONFIG['rate_limit_seconds']:
        return False
    ip_rate_limit[ip_address] = current_time
    return True

def start_vpn_service():
    init_wireguard_server()
    host = os.getenv('VPN_SERVICE_HOST', '127.0.0.1')
    port = int(os.getenv('VPN_SERVICE_PORT', '5008'))
    print(f"啟動 VPN 服務:")
    print(f"  - 主機: {host}")
    print(f"  - 端口: {port}")
    print(f"  - WireGuard 端口: {WIREGUARD_CONFIG['server_port']}")
    print(f"  - 配置目錄: {STORAGE_CONFIG['config_dir']}")
    print(f"  - 日誌目錄: {STORAGE_CONFIG['log_dir']}")
    print(f"  - 多線程支援: 是")
    if not os.path.exists(STORAGE_CONFIG['config_dir']):
        print(f"警告: 配置目錄不存在，正在創建: {STORAGE_CONFIG['config_dir']}")
        os.makedirs(STORAGE_CONFIG['config_dir'], exist_ok=True)
    server = ThreadedHTTPServer((host, port), VPNServiceHandler)
    try:
        print(f"VPN 服務已啟動，監聽 {host}:{port}")
        server.serve_forever()
    except KeyboardInterrupt:
        print("\n正在關閉 VPN 服務...")
        server.shutdown()
        print("VPN 服務已關閉")

if __name__ == '__main__':
    start_vpn_service()
