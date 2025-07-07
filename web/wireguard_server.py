import subprocess
import ipaddress
import json
import os
from pathlib import Path
from typing import Dict, List, Optional
import threading
import time
import platform
import shutil
import logging
from datetime import datetime

class WireGuardServer:
    def __init__(self, interface_name: str = "wg0", server_port: int = 51820, 
                 network: str = "10.0.0.0/24"):
        self.interface_name = interface_name
        self.server_port = server_port
        self.network = ipaddress.IPv4Network(network)
        self.server_ip = str(list(self.network.hosts())[0])
        
        # 跨平台路徑處理
        if platform.system() == "Windows":
            self.config_dir = Path("d:/hivemind/wireguard_configs")
        else:
            # Linux環境，使用當前目錄或標準配置目錄
            current_dir = Path.cwd()
            if current_dir.is_relative_to(Path("/mnt")):
                # 如果在掛載目錄中，使用當前目錄
                self.config_dir = current_dir / "wireguard_configs"
            else:
                # 否則使用標準配置目錄
                self.config_dir = Path("/etc/wireguard")
                
        # 確保配置目錄存在
        try:
            self.config_dir.mkdir(parents=True, exist_ok=True)
            print(f"配置目錄: {self.config_dir}")
        except PermissionError:
            # 如果沒有權限寫入/etc/wireguard，使用用戶目錄
            self.config_dir = Path.home() / ".config" / "wireguard"
            self.config_dir.mkdir(parents=True, exist_ok=True)
            print(f"配置目錄 (用戶): {self.config_dir}")
        
        self.clients: Dict[str, dict] = {}
        self.next_client_ip = 2  # 從 .2 開始分配
        self.server_private_key = None
        self.server_public_key = None
        
        # 檢查WireGuard工具 - 在Linux上可能不需要
        self.wg_path, self.wg_quick_path = self._find_wireguard_tools()
        
        # 設置日誌系統
        self._setup_logging()
        
        # 連接監控狀態
        self.monitoring_active = False
        self.connection_log = []
        
        # 只允許主進程做底層初始化
        is_gunicorn_worker = "gunicorn" in os.environ.get("SERVER_SOFTWARE", "")
        is_main_process = os.getpid() == os.getppid() or os.environ.get("IS_WG_MASTER") == "1"
        do_init = not is_gunicorn_worker or is_main_process
        if do_init:
            self._load_or_generate_server_keys()
            self._setup_firewall()
            self._setup_server()
            self._start_connection_monitoring()
        
    def _find_wireguard_tools(self):
        """尋找WireGuard工具路徑"""
        wg_candidates = []
        wg_quick_candidates = []
        
        if platform.system() == "Windows":
            # Windows常見安裝路徑
            common_paths = [
                "C:/Program Files/WireGuard",
                "C:/Program Files (x86)/WireGuard",
                os.path.expanduser("~/AppData/Local/WireGuard"),
            ]
            
            for base_path in common_paths:
                wg_candidates.extend([
                    f"{base_path}/wg.exe",
                    f"{base_path}/bin/wg.exe",
                ])
                wg_quick_candidates.extend([
                    f"{base_path}/wg-quick.exe",
                    f"{base_path}/bin/wg-quick.exe",
                ])
        else:
            # Linux/Unix路徑
            wg_candidates = ["wg", "/usr/bin/wg", "/usr/local/bin/wg"]
            wg_quick_candidates = ["wg-quick", "/usr/bin/wg-quick", "/usr/local/bin/wg-quick"]
        
        # 尋找可用的工具
        wg_path = None
        wg_quick_path = None
        
        for candidate in wg_candidates:
            if shutil.which(candidate) or (os.path.exists(candidate) and os.access(candidate, os.X_OK)):
                wg_path = candidate
                break
        
        for candidate in wg_quick_candidates:
            if shutil.which(candidate) or (os.path.exists(candidate) and os.access(candidate, os.X_OK)):
                wg_quick_path = candidate
                break
        
        print(f"WireGuard工具檢測: wg={wg_path}, wg-quick={wg_quick_path}")
        return wg_path, wg_quick_path
        
    def _load_or_generate_server_keys(self):
        """載入或生成伺服器密鑰"""
        private_key_file = self.config_dir / "server_private.key"
        public_key_file = self.config_dir / "server_public.key"
        
        if private_key_file.exists() and public_key_file.exists():
            self.server_private_key = private_key_file.read_text().strip()
            self.server_public_key = public_key_file.read_text().strip()
            print("使用現有的伺服器密鑰")
        else:
            try:
                if self.wg_path:
                    self.server_private_key = self._generate_private_key()
                    self.server_public_key = self._generate_public_key(self.server_private_key)
                else:
                    print("未找到WireGuard工具，使用Python生成密鑰")
                    self._generate_keys_fallback()
                
                private_key_file.write_text(self.server_private_key)
                public_key_file.write_text(self.server_public_key)
                print("伺服器密鑰已生成並保存")
            except Exception as e:
                print(f"生成密鑰失敗: {e}")
                # 使用Python實現的密鑰生成作為備用方案
                self._generate_keys_fallback()
                
    def _generate_private_key(self) -> str:
        """生成私鑰"""
        if not self.wg_path:
            raise RuntimeError("WireGuard工具不可用")
            
        result = subprocess.run([self.wg_path, 'genkey'], capture_output=True, text=True, check=True)
        return result.stdout.strip()
        
    def _generate_public_key(self, private_key: str) -> str:
        """從私鑰生成公鑰"""
        if not self.wg_path:
            raise RuntimeError("WireGuard工具不可用")
            
        result = subprocess.run([self.wg_path, 'pubkey'], input=private_key, 
                              capture_output=True, text=True, check=True)
        return result.stdout.strip()
        
    def _generate_keys_fallback(self):
        """備用密鑰生成方法（使用Python）"""
        try:
            from cryptography.hazmat.primitives import serialization
            from cryptography.hazmat.primitives.asymmetric import x25519
            import base64
            
            # 生成私鑰
            private_key_obj = x25519.X25519PrivateKey.generate()
            private_key_bytes = private_key_obj.private_bytes(
                encoding=serialization.Encoding.Raw,
                format=serialization.PrivateFormat.Raw,
                encryption_algorithm=serialization.NoEncryption()
            )
            self.server_private_key = base64.b64encode(private_key_bytes).decode()
            
            # 生成公鑰
            public_key_obj = private_key_obj.public_key()
            public_key_bytes = public_key_obj.public_bytes(
                encoding=serialization.Encoding.Raw,
                format=serialization.PublicFormat.Raw
            )
            self.server_public_key = base64.b64encode(public_key_bytes).decode()
            
            # 保存密鑰
            private_key_file = self.config_dir / "server_private.key"
            public_key_file = self.config_dir / "server_public.key"
            private_key_file.write_text(self.server_private_key)
            public_key_file.write_text(self.server_public_key)
            
            print("使用Python備用方法生成密鑰")
            
        except ImportError:
            print("錯誤：需要安裝cryptography庫")
            if platform.system() == "Linux":
                print("Linux安裝命令:")
                print("  pip3 install cryptography --break-system-packages")
                print("  或: apt install python3-cryptography")
            else:
                print("  pip install cryptography")
            raise RuntimeError("缺少cryptography庫")
        
    def _setup_server(self):
        """設置WireGuard伺服器"""
        if platform.system() == "Windows":
            print("Windows環境：請手動配置WireGuard服務")
            self._setup_windows_config()
        else:
            self._setup_linux_config()
            
    def _setup_windows_config(self):
        """Windows環境配置"""
        config_content = f"""[Interface]
PrivateKey = {self.server_private_key}
Address = {self.server_ip}/24
ListenPort = {self.server_port}
DNS = 8.8.8.8

# 請手動啟動WireGuard服務
"""
        
        config_file = self.config_dir / f"{self.interface_name}.conf"
        config_file.write_text(config_content)
        print(f"Windows配置文件已創建: {config_file}")
        print("請使用WireGuard GUI手動導入配置文件")
        
    def _setup_linux_config(self):
        """Linux環境配置"""
        # 檢測網路介面
        network_interface = self._detect_network_interface()
        
        config_content = f"""[Interface]
PrivateKey = {self.server_private_key}
Address = {self.server_ip}/24
ListenPort = {self.server_port}
PostUp = iptables -A FORWARD -i {self.interface_name} -j ACCEPT; iptables -t nat -A POSTROUTING -o {network_interface} -j MASQUERADE; ip6tables -A FORWARD -i {self.interface_name} -j ACCEPT; ip6tables -t nat -A POSTROUTING -o {network_interface} -j MASQUERADE
PostDown = iptables -D FORWARD -i {self.interface_name} -j ACCEPT; iptables -t nat -D POSTROUTING -o {network_interface} -j MASQUERADE; ip6tables -D FORWARD -i {self.interface_name} -j ACCEPT; ip6tables -t nat -D POSTROUTING -o {network_interface} -j MASQUERADE

"""
        
        config_file = self.config_dir / f"{self.interface_name}.conf"
        config_file.write_text(config_content)
        print(f"Linux配置文件已創建: {config_file}")
        
        # 啟動WireGuard介面
        if self.wg_quick_path:
            try:
                # 檢查是否有權限
                if os.geteuid() != 0:
                    print("警告：需要root權限來啟動WireGuard介面")
                    print(f"請使用 sudo 運行或手動執行: sudo {self.wg_quick_path} up {config_file}")
                    return
                
                subprocess.run([self.wg_quick_path, 'down', str(config_file)], 
                              capture_output=True, check=False)
                result = subprocess.run([self.wg_quick_path, 'up', str(config_file)], 
                              capture_output=True, check=True)
                print(f"WireGuard伺服器已啟動在 {self.interface_name}")
            except subprocess.CalledProcessError as e:
                print(f"啟動WireGuard失敗: {e}")
                if e.stderr:
                    print(f"錯誤詳情: {e.stderr.decode()}")
                print("您可能需要:")
                print("1. 安裝WireGuard: apt install wireguard")
                print("2. 使用root權限運行")
                print(f"3. 手動執行: sudo wg-quick up {config_file}")
        else:
            print("找不到wg-quick工具")
            print("請安裝WireGuard: apt install wireguard")
            
    def _detect_network_interface(self):
        """檢測主要網路介面"""
        try:
            # 嘗試找到預設路由的介面
            result = subprocess.run(['ip', 'route', 'show', 'default'], 
                                  capture_output=True, text=True, check=True)
            
            for line in result.stdout.split('\n'):
                if 'default via' in line:
                    parts = line.split()
                    if 'dev' in parts:
                        idx = parts.index('dev') + 1
                        if idx < len(parts):
                            return parts[idx]
                            
        except (subprocess.CalledProcessError, FileNotFoundError):
            pass
            
        # 備用方案：常見介面名稱
        common_interfaces = ['eth0', 'enp0s3', 'ens18', 'wlan0']
        for iface in common_interfaces:
            try:
                result = subprocess.run(['ip', 'link', 'show', iface], 
                                      capture_output=True, check=True)
                return iface
            except subprocess.CalledProcessError:
                continue
                
        return 'eth0'  # 最後的備用方案            
    def add_client(self, client_name: str) -> dict:
        """動態添加客戶端"""
        if client_name in self.clients:
            return self.clients[client_name]
            
        # 生成客戶端密鑰
        try:
            if self.wg_path:
                client_private_key = self._generate_private_key()
                client_public_key = self._generate_public_key(client_private_key)
            else:
                client_private_key, client_public_key = self._generate_client_keys_fallback()
        except Exception as e:
            print(f"生成客戶端密鑰失敗，使用備用方法: {e}")
            client_private_key, client_public_key = self._generate_client_keys_fallback()
        
        # 分配IP
        if self.next_client_ip >= len(list(self.network.hosts())):
            raise RuntimeError("IP地址池已耗盡")
            
        client_ip = str(list(self.network.hosts())[self.next_client_ip])
        self.next_client_ip += 1
        
        # 獲取伺服器公網IP
        server_endpoint = self._get_server_endpoint()
        
        client_config = {
            'name': client_name,
            'private_key': client_private_key,
            'public_key': client_public_key,
            'ip': client_ip,
            'server_public_key': self.server_public_key,
            'server_endpoint': server_endpoint
        }
        
        self.clients[client_name] = client_config
        
        # 動態添加到WireGuard配置
        if platform.system() != "Windows" and self.wg_path:
            try:
                subprocess.run([self.wg_path, 'set', self.interface_name, 'peer', client_public_key,
                               'allowed-ips', f'{client_ip}/32'], check=True)
                print(f"客戶端 {client_name} 已添加到WireGuard")
            except subprocess.CalledProcessError as e:
                print(f"添加客戶端到WireGuard失敗: {e}")
                print("客戶端配置已創建，請手動重啟WireGuard服務")
        else:
            print(f"客戶端 {client_name} 配置已創建")
            
        return client_config
        
    def _get_server_endpoint(self):
        """獲取伺服器端點"""
        try:
            # 嘗試獲取公網IP
            import urllib.request
            
            services = [
                "https://ipv4.icanhazip.com",
                "https://api.ipify.org",
                "http://checkip.amazonaws.com"
            ]
            
            for service in services:
                try:
                    response = urllib.request.urlopen(service, timeout=5)
                    public_ip = response.read().decode().strip()
                    if self._is_valid_ip(public_ip):
                        return f"{public_ip}:{self.server_port}"
                except:
                    continue
                    
        except Exception:
            pass
            
        # 備用方案：使用本地IP或佔位符
        return f"YOUR_SERVER_IP:{self.server_port}"
        
    def _is_valid_ip(self, ip):
        """驗證IP地址格式"""
        try:
            parts = ip.split('.')
            return len(parts) == 4 and all(0 <= int(part) <= 255 for part in parts)
        except:
            return False
        
    def remove_client(self, client_name: str):
        """移除客戶端"""
        if client_name not in self.clients:
            return False
            
        client_config = self.clients[client_name]
        
        if platform.system() != "Windows" and self.wg_path:
            try:
                subprocess.run([self.wg_path, 'set', self.interface_name, 'peer', 
                               client_config['public_key'], 'remove'], check=True)
                print(f"客戶端 {client_name} 已從WireGuard移除")
            except subprocess.CalledProcessError as e:
                print(f"從WireGuard移除客戶端失敗: {e}")
        
        del self.clients[client_name]
        print(f"客戶端 {client_name} 已移除")
        return True
        
    def get_client_config(self, client_name: str) -> Optional[str]:
        """獲取客戶端配置文件內容"""
        if client_name not in self.clients:
            return None
            
        client = self.clients[client_name]
        config = f"""[Interface]
PrivateKey = {client['private_key']}
Address = {client['ip']}/24
DNS = 8.8.8.8

[Peer]
PublicKey = {client['server_public_key']}
Endpoint = {client['server_endpoint']}
AllowedIPs = 0.0.0.0/0
PersistentKeepalive = 25
"""
        return config
        
    def save_client_config(self, client_name: str):
        """保存客戶端配置文件"""
        config_content = self.get_client_config(client_name)
        if config_content:
            config_file = self.config_dir / f"{client_name}.conf"
            config_file.write_text(config_content)
            print(f"客戶端配置已保存到: {config_file}")
            
    def list_clients(self) -> List[str]:
        """列出所有客戶端"""
        return list(self.clients.keys())
    
    def _setup_firewall(self):
        """設置防火牆規則"""
        if platform.system() == "Windows":
            self._setup_windows_firewall()
        else:
            self._setup_linux_firewall()
            
    def _setup_windows_firewall(self):
        """設置Windows防火牆"""
        print("Windows防火牆設置:")
        print("請手動在Windows防火牆中添加以下規則:")
        print(f"1. 允許入站連接端口 {self.server_port}/UDP (WireGuard)")
        print("2. 允許WireGuard應用程式通過防火牆")
        print("3. 在高級設置中允許VPN流量轉發")
        
    def _setup_linux_firewall(self):
        """設置Linux防火牆規則"""
        print("正在配置Linux防火牆...")
        
        # 檢查防火牆類型
        firewall_type = self._detect_firewall()
        print(f"檢測到防火牆類型: {firewall_type}")
        
        if firewall_type == "ufw":
            self._setup_ufw_rules()
        elif firewall_type == "iptables":
            self._setup_iptables_rules()
        elif firewall_type == "firewalld":
            self._setup_firewalld_rules()
        else:
            self._show_manual_firewall_rules()
            
    def _detect_firewall(self):
        """檢測Linux防火牆類型"""
        # 檢查UFW
        try:
            result = subprocess.run(['ufw', 'status'], capture_output=True, check=True)
            return "ufw"
        except (subprocess.CalledProcessError, FileNotFoundError):
            pass
            
        # 檢查firewalld
        try:
            result = subprocess.run(['firewall-cmd', '--state'], capture_output=True, check=True)
            return "firewalld"
        except (subprocess.CalledProcessError, FileNotFoundError):
            pass
            
        # 檢查iptables
        try:
            result = subprocess.run(['iptables', '-L'], capture_output=True, check=True)
            return "iptables"
        except (subprocess.CalledProcessError, FileNotFoundError):
            pass
            
        return "unknown"
        
    def _setup_ufw_rules(self):
        """設置UFW防火牆規則"""
        try:
            commands = [
                f"ufw allow {self.server_port}/udp comment 'WireGuard'",
                "ufw allow ssh",
                f"ufw route allow in on {self.interface_name} out on {self._detect_network_interface()}",
                f"ufw route allow in on {self._detect_network_interface()} out on {self.interface_name}"
            ]
            
            for cmd in commands:
                print(f"執行: {cmd}")
                result = subprocess.run(cmd.split(), capture_output=True, text=True)
                if result.returncode != 0:
                    print(f"警告: {cmd} 執行失敗: {result.stderr}")
                else:
                    print("✓ 成功")
                    
            # 啟用UFW
            print("啟用UFW...")
            subprocess.run(['ufw', '--force', 'enable'], capture_output=True)
            print("✓ UFW防火牆規則已設置")
            
        except Exception as e:
            print(f"UFW設置失敗: {e}")
            self._show_manual_firewall_rules()
            
    def _setup_iptables_rules(self):
        """設置iptables防火牆規則"""
        try:
            network_interface = self._detect_network_interface()
            
            rules = [
                # 允許WireGuard端口
                f"iptables -A INPUT -p udp --dport {self.server_port} -j ACCEPT",
                
                # 允許已建立的連接
                "iptables -A INPUT -m conntrack --ctstate RELATED,ESTABLISHED -j ACCEPT",
                
                # 允許本地回環
                "iptables -A INPUT -i lo -j ACCEPT",
                
                # 允許SSH (重要!)
                "iptables -A INPUT -p tcp --dport 22 -j ACCEPT",
                
                # WireGuard轉發規則
                f"iptables -A FORWARD -i {self.interface_name} -j ACCEPT",
                f"iptables -A FORWARD -o {self.interface_name} -j ACCEPT",
                
                # NAT規則
                f"iptables -t nat -A POSTROUTING -s {self.network} -o {network_interface} -j MASQUERADE",
                
                # IPv6規則
                f"ip6tables -A FORWARD -i {self.interface_name} -j ACCEPT",
                f"ip6tables -A FORWARD -o {self.interface_name} -j ACCEPT",
            ]
            
            for rule in rules:
                print(f"執行: {rule}")
                result = subprocess.run(rule.split(), capture_output=True, text=True)
                if result.returncode != 0:
                    print(f"警告: 規則執行失敗: {result.stderr}")
                else:
                    print("✓ 成功")
                    
            # 保存規則
            self._save_iptables_rules()
            print("✓ iptables防火牆規則已設置")
            
        except Exception as e:
            print(f"iptables設置失敗: {e}")
            self._show_manual_firewall_rules()
            
    def _setup_firewalld_rules(self):
        """設置firewalld防火牆規則"""
        try:
            commands = [
                f"firewall-cmd --permanent --add-port={self.server_port}/udp",
                "firewall-cmd --permanent --add-service=ssh",
                f"firewall-cmd --permanent --add-interface={self.interface_name} --zone=trusted",
                "firewall-cmd --permanent --add-masquerade",
                "firewall-cmd --reload"
            ]
            
            for cmd in commands:
                print(f"執行: {cmd}")
                result = subprocess.run(cmd.split(), capture_output=True, text=True)
                if result.returncode != 0:
                    print(f"警告: {cmd} 執行失敗: {result.stderr}")
                else:
                    print("✓ 成功")
                    
            print("✓ firewalld防火牆規則已設置")
            
        except Exception as e:
            print(f"firewalld設置失敗: {e}")
            self._show_manual_firewall_rules()
            
    def _save_iptables_rules(self):
        """保存iptables規則"""
        try:
            # 嘗試不同的保存方法
            save_commands = [
                "iptables-save > /etc/iptables/rules.v4",
                "ip6tables-save > /etc/iptables/rules.v6",
                "netfilter-persistent save",
                "service iptables save"
            ]
            
            for cmd in save_commands:
                try:
                    subprocess.run(cmd, shell=True, capture_output=True, check=True)
                    print(f"✓ 規則已保存: {cmd}")
                    break
                except subprocess.CalledProcessError:
                    continue
                    
        except Exception as e:
            print(f"保存iptables規則失敗: {e}")
            print("請手動保存規則以確保重啟後生效")
            
    def _show_manual_firewall_rules(self):
        """顯示手動防火牆配置指令"""
        network_interface = self._detect_network_interface()
        
        print("\n=== 手動防火牆配置 ===")
        print("請根據您的防火牆類型執行以下命令:")
        print()
        
        print("## UFW防火牆:")
        print(f"sudo ufw allow {self.server_port}/udp")
        print("sudo ufw allow ssh")
        print(f"sudo ufw route allow in on {self.interface_name}")
        print("sudo ufw --force enable")
        print()
        
        print("## iptables防火牆:")
        print(f"sudo iptables -A INPUT -p udp --dport {self.server_port} -j ACCEPT")
        print("sudo iptables -A INPUT -m conntrack --ctstate RELATED,ESTABLISHED -j ACCEPT")
        print("sudo iptables -A INPUT -i lo -j ACCEPT")
        print("sudo iptables -A INPUT -p tcp --dport 22 -j ACCEPT")
        print(f"sudo iptables -A FORWARD -i {self.interface_name} -j ACCEPT")
        print(f"sudo iptables -A FORWARD -o {self.interface_name} -j ACCEPT")
        print(f"sudo iptables -t nat -A POSTROUTING -s {self.network} -o {network_interface} -j MASQUERADE")
        print("sudo iptables-save > /etc/iptables/rules.v4")
        print()
        
        print("## firewalld防火牆:")
        print(f"sudo firewall-cmd --permanent --add-port={self.server_port}/udp")
        print("sudo firewall-cmd --permanent --add-service=ssh")
        print(f"sudo firewall-cmd --permanent --add-interface={self.interface_name} --zone=trusted")
        print("sudo firewall-cmd --permanent --add-masquerade")
        print("sudo firewall-cmd --reload")
        print()
        
        print("## 雲服務商安全組設置:")
        print(f"- 入站規則: 允許 UDP {self.server_port} (所有IP或指定IP)")
        print("- 出站規則: 允許所有流量")
        print()
        
    def check_firewall_status(self):
        """檢查防火牆狀態"""
        print("=== 防火牆狀態檢查 ===")
        
        # 檢查端口是否開放
        print(f"檢查端口 {self.server_port}/UDP...")
        try:
            # 使用netstat檢查監聽端口
            result = subprocess.run(['netstat', '-ulnp'], capture_output=True, text=True)
            if f":{self.server_port}" in result.stdout:
                print(f"✓ 端口 {self.server_port} 正在監聽")
            else:
                print(f"⚠ 端口 {self.server_port} 未在監聽")
        except FileNotFoundError:
            print("netstat 不可用，跳過端口檢查")
            
        # 檢查防火牆狀態
        firewall_type = self._detect_firewall()
        if firewall_type == "ufw":
            try:
                result = subprocess.run(['ufw', 'status'], capture_output=True, text=True)
                print("UFW狀態:")
                print(result.stdout)
            except:
                pass
        elif firewall_type == "firewalld":
            try:
                result = subprocess.run(['firewall-cmd', '--list-all'], capture_output=True, text=True)
                print("firewalld狀態:")
                print(result.stdout)
            except:
                pass
                
    def enable_ip_forwarding(self):
        """啟用IP轉發"""
        print("啟用IP轉發...")
        try:
            # 臨時啟用
            subprocess.run(['sysctl', '-w', 'net.ipv4.ip_forward=1'], check=True)
            subprocess.run(['sysctl', '-w', 'net.ipv6.conf.all.forwarding=1'], check=True)
            
            # 永久啟用
            sysctl_conf = Path('/etc/sysctl.conf')
            if sysctl_conf.exists():
                content = sysctl_conf.read_text()
                if 'net.ipv4.ip_forward=1' not in content:
                    with open(sysctl_conf, 'a') as f:
                        f.write('\n# WireGuard IP forwarding\n')
                        f.write('net.ipv4.ip_forward=1\n')
                        f.write('net.ipv6.conf.all.forwarding=1\n')
                    print("✓ IP轉發已永久啟用")
                else:
                    print("✓ IP轉發已經啟用")
            else:
                print("⚠ 無法找到 /etc/sysctl.conf，請手動啟用IP轉發")
                
        except subprocess.CalledProcessError as e:
            print(f"啟用IP轉發失敗: {e}")
            print("請手動執行:")
            print("echo 'net.ipv4.ip_forward=1' >> /etc/sysctl.conf")
            print("echo 'net.ipv6.conf.all.forwarding=1' >> /etc/sysctl.conf")
            print("sysctl -p")
        
    def _setup_logging(self):
        """設置日誌系統"""
        # 創建日誌目錄
        log_dir = self.config_dir / "logs"
        log_dir.mkdir(exist_ok=True)
        
        # 設置日誌文件
        log_file = log_dir / f"wireguard_server_{datetime.now().strftime('%Y%m%d')}.log"
        
        # 配置日誌格式
        logging.basicConfig(
            level=logging.INFO,
            format='%(asctime)s - %(levelname)s - %(message)s',
            handlers=[
                logging.FileHandler(log_file, encoding='utf-8'),
                logging.StreamHandler()  # 同時輸出到控制台
            ]
        )
        
        self.logger = logging.getLogger(__name__)
        self.logger.info("WireGuard伺服器日誌系統已啟動")
        print(f"日誌文件: {log_file}")
        
    def _start_connection_monitoring(self):
        """啟動連接監控"""
        if platform.system() != "Windows" and self.wg_path:
            self.monitoring_active = True
            monitor_thread = threading.Thread(target=self._monitor_connections)
            monitor_thread.daemon = True
            monitor_thread.start()
            self.logger.info("連接監控已啟動")
        else:
            self.logger.info("Windows環境或工具不可用，跳過自動監控")
            
    def _monitor_connections(self):
        """監控客戶端連接狀態"""
        last_peer_info = {}
        
        while self.monitoring_active:
            try:
                if not self.wg_path:
                    time.sleep(30)
                    continue
                    
                # 獲取WireGuard狀態
                result = subprocess.run([self.wg_path, 'show', self.interface_name, 'dump'], 
                                      capture_output=True, text=True)
                
                if result.returncode != 0:
                    time.sleep(30)
                    continue
                    
                current_peer_info = {}
                lines = result.stdout.strip().split('\n')
                
                for line in lines[1:]:  # 跳過標題行
                    if not line.strip():
                        continue
                        
                    parts = line.split('\t')
                    if len(parts) >= 6:
                        public_key = parts[0]
                        endpoint = parts[2] if parts[2] != '(none)' else None
                        latest_handshake = int(parts[4]) if parts[4] != '0' else 0
                        transfer_rx = int(parts[5])
                        transfer_tx = int(parts[6]) if len(parts) > 6 else 0
                        
                        current_peer_info[public_key] = {
                            'endpoint': endpoint,
                            'latest_handshake': latest_handshake,
                            'transfer_rx': transfer_rx,
                            'transfer_tx': transfer_tx,
                            'last_seen': datetime.fromtimestamp(latest_handshake) if latest_handshake > 0 else None
                        }
                
                # 檢查新連接和斷開的客戶端
                self._check_connection_changes(last_peer_info, current_peer_info)
                last_peer_info = current_peer_info.copy()
                
                time.sleep(10)  # 每10秒檢查一次
                
            except Exception as e:
                self.logger.error(f"監控過程出錯: {e}")
                time.sleep(30)
                
    def _check_connection_changes(self, last_info, current_info):
        """檢查連接變化"""
        current_time = datetime.now()
        
        # 檢查新連接
        for public_key, info in current_info.items():
            client_name = self._get_client_name_by_public_key(public_key)
            
            if public_key not in last_info:
                # 新的客戶端連接
                if info['latest_handshake'] > 0:
                    self._log_client_connection(client_name, public_key, info, "CONNECTED")
            else:
                last_handshake = last_info[public_key]['latest_handshake']
                current_handshake = info['latest_handshake']
                
                # 檢查是否有新的握手（表示重新連接）
                if current_handshake > last_handshake and current_handshake > 0:
                    self._log_client_connection(client_name, public_key, info, "RECONNECTED")
                
                # 檢查數據傳輸
                last_rx = last_info[public_key]['transfer_rx']
                last_tx = last_info[public_key]['transfer_tx']
                current_rx = info['transfer_rx']
                current_tx = info['transfer_tx']
                
                if current_rx > last_rx or current_tx > last_tx:
                    rx_diff = current_rx - last_rx
                    tx_diff = current_tx - last_tx
                    self._log_data_transfer(client_name, public_key, rx_diff, tx_diff)
        
        # 檢查斷開的連接
        for public_key in last_info:
            if public_key not in current_info:
                client_name = self._get_client_name_by_public_key(public_key)
                self._log_client_connection(client_name, public_key, last_info[public_key], "DISCONNECTED")
                
    def _get_client_name_by_public_key(self, public_key):
        """根據公鑰獲取客戶端名稱"""
        for name, config in self.clients.items():
            if config['public_key'] == public_key:
                return name
        return f"Unknown({public_key[:8]}...)"
        
    def _log_client_connection(self, client_name, public_key, info, status):
        """記錄客戶端連接日誌"""
        endpoint = info.get('endpoint', 'Unknown')
        last_seen = info.get('last_seen')
        
        log_entry = {
            'timestamp': datetime.now().isoformat(),
            'client_name': client_name,
            'public_key': public_key[:16] + "...",
            'status': status,
            'endpoint': endpoint,
            'last_seen': last_seen.isoformat() if last_seen else None
        }
        
        self.connection_log.append(log_entry)
        
        # 保持日誌條目不超過1000條
        if len(self.connection_log) > 1000:
            self.connection_log = self.connection_log[-500:]
            
        # 記錄到日誌文件
        if status == "CONNECTED":
            self.logger.info(f"客戶端連接: {client_name} ({endpoint}) - 首次握手")
            print(f"✓ 客戶端 {client_name} 已連接 (來源: {endpoint})")
        elif status == "RECONNECTED":
            self.logger.info(f"客戶端重連: {client_name} ({endpoint}) - 新握手")
            print(f"↻ 客戶端 {client_name} 重新連接 (來源: {endpoint})")
        elif status == "DISCONNECTED":
            self.logger.info(f"客戶端斷開: {client_name} - 已離線")
            print(f"✗ 客戶端 {client_name} 已斷開連接")
            
    def _log_data_transfer(self, client_name, public_key, rx_bytes, tx_bytes):
        """記錄數據傳輸日誌（僅當傳輸量較大時）"""
        if rx_bytes > 1024 or tx_bytes > 1024:  # 只記錄大於1KB的傳輸
            rx_mb = rx_bytes / (1024 * 1024)
            tx_mb = tx_bytes / (1024 * 1024)
            
            if rx_mb > 0.1 or tx_mb > 0.1:  # 只記錄大於0.1MB的傳輸
                self.logger.debug(f"數據傳輸: {client_name} - 下載: {rx_mb:.2f}MB, 上傳: {tx_mb:.2f}MB")
                
    def get_connection_status(self):
        """獲取當前連接狀態"""
        if platform.system() == "Windows" or not self.wg_path:
            print("Windows環境或工具不可用，無法獲取實時狀態")
            return
            
        try:
            result = subprocess.run([self.wg_path, 'show', self.interface_name], 
                                  capture_output=True, text=True)
            
            if result.returncode != 0:
                print("無法獲取WireGuard狀態")
                return
                
            print("=== 當前連接狀態 ===")
            print(result.stdout)
            
            # 獲取詳細的peer信息
            result = subprocess.run([self.wg_path, 'show', self.interface_name, 'dump'], 
                                  capture_output=True, text=True)
            
            if result.returncode == 0:
                lines = result.stdout.strip().split('\n')
                if len(lines) > 1:
                    print("\n=== 客戶端詳細信息 ===")
                    for line in lines[1:]:
                        if not line.strip():
                            continue
                            
                        parts = line.split('\t')
                        if len(parts) >= 6:
                            public_key = parts[0]
                            endpoint = parts[2] if parts[2] != '(none)' else "未知"
                            latest_handshake = int(parts[4]) if parts[4] != '0' else 0
                            transfer_rx = int(parts[5])
                            transfer_tx = int(parts[6]) if len(parts) > 6 else 0
                            
                            client_name = self._get_client_name_by_public_key(public_key)
                            last_seen = datetime.fromtimestamp(latest_handshake) if latest_handshake > 0 else "從未連接"
                            
                            rx_mb = transfer_rx / (1024 * 1024)
                            tx_mb = transfer_tx / (1024 * 1024)
                            
                            print(f"客戶端: {client_name}")
                            print(f"  公鑰: {public_key[:16]}...")
                            print(f"  端點: {endpoint}")
                            print(f"  最後握手: {last_seen}")
                            print(f"  傳輸量: ↓{rx_mb:.2f}MB ↑{tx_mb:.2f}MB")
                            print()
                            
        except Exception as e:
            self.logger.error(f"獲取連接狀態失敗: {e}")
            
    def show_connection_log(self, lines=20):
        """顯示連接日誌"""
        print(f"=== 最近 {lines} 條連接日誌 ===")
        
        if not self.connection_log:
            print("暫無連接日誌")
            return
            
        recent_logs = self.connection_log[-lines:]
        
        for entry in recent_logs:
            timestamp = datetime.fromisoformat(entry['timestamp']).strftime('%Y-%m-%d %H:%M:%S')
            status_icon = {
                'CONNECTED': '✓',
                'RECONNECTED': '↻',
                'DISCONNECTED': '✗'
            }.get(entry['status'], '?')
            
            print(f"{timestamp} {status_icon} {entry['client_name']} - {entry['status']} ({entry['endpoint']})")
            
    def export_logs(self, filename=None):
        """導出連接日誌到JSON文件"""
        if filename is None:
            filename = self.config_dir / "logs" / f"connection_log_{datetime.now().strftime('%Y%m%d_%H%M%S')}.json"
        else:
            filename = Path(filename)
            
        try:
            with open(filename, 'w', encoding='utf-8') as f:
                json.dump(self.connection_log, f, indent=2, ensure_ascii=False)
                
            print(f"連接日誌已導出到: {filename}")
            self.logger.info(f"連接日誌已導出到: {filename}")
            
        except Exception as e:
            print(f"導出日誌失敗: {e}")
            self.logger.error(f"導出日誌失敗: {e}")

if __name__ == "__main__":
    try:
        print(f"運行平台: {platform.system()}")
        print(f"當前目錄: {Path.cwd()}")
        print(f"用戶權限: {'root' if os.geteuid() == 0 else '普通用戶'}")
        
        server = WireGuardServer()
        
        # 啟用IP轉發
        server.enable_ip_forwarding()
        
        # 示例：添加客戶端
        client_config = server.add_client("test_client")
        server.save_client_config("test_client")
        
        print("\n=== WireGuard 伺服器已啟動 ===")
        print("可用命令:")
        print("- add <client_name>  : 添加客戶端")
        print("- remove <client_name>: 移除客戶端")
        print("- list              : 列出客戶端")
        print("- status            : 顯示狀態")
        print("- connections       : 顯示當前連接狀態")
        print("- logs [lines]      : 顯示連接日誌 (預設20行)")
        print("- export            : 導出連接日誌")
        print("- firewall          : 檢查防火牆狀態")
        print("- quit              : 退出")
        print("按Ctrl+C也可退出")
        
        while True:
            try:
                command = input("\n輸入命令: ").strip().split()
                if not command:
                    continue
                    
                if command[0] == "add" and len(command) > 1:
                    client_name = command[1]
                    client_config = server.add_client(client_name)
                    server.save_client_config(client_name)
                    print(f"✓ 客戶端 {client_name} 已添加")
                    print(f"  IP: {client_config['ip']}")
                    print(f"  配置文件: {server.config_dir}/{client_name}.conf")
                    
                elif command[0] == "remove" and len(command) > 1:
                    client_name = command[1]
                    if server.remove_client(client_name):
                        print(f"✓ 客戶端 {client_name} 已移除")
                    else:
                        print(f"✗ 客戶端 {client_name} 不存在")
                    
                elif command[0] == "list":
                    clients = server.list_clients()
                    if clients:
                        print("當前客戶端:")
                        for client in clients:
                            client_info = server.clients[client]
                            print(f"  - {client}: {client_info['ip']}")
                    else:
                        print("沒有客戶端")
                        
                elif command[0] == "connections":
                    server.get_connection_status()
                    
                elif command[0] == "logs":
                    lines = int(command[1]) if len(command) > 1 and command[1].isdigit() else 20
                    server.show_connection_log(lines)
                    
                elif command[0] == "export":
                    server.export_logs()
                        
                elif command[0] == "firewall":
                    server.check_firewall_status()
                    
                elif command[0] == "status":
                    print(f"伺服器IP: {server.server_ip}")
                    print(f"監聽端口: {server.server_port}")
                    print(f"配置目錄: {server.config_dir}")
                    print(f"客戶端數量: {len(server.clients)}")
                    print(f"網路介面: {server._detect_network_interface()}")
                    print(f"監控狀態: {'運行中' if server.monitoring_active else '未啟動'}")
                    
                elif command[0] == "quit":
                    server.monitoring_active = False
                    break
                    
                else:
                    print("未知命令，請使用 add/remove/list/status/connections/logs/export/firewall/quit")
                    
            except EOFError:
                break
            except KeyboardInterrupt:
                break
                
    except Exception as e:
        print(f"啟動失敗: {e}")
        print("\n可能的解決方案:")
        print("1. 安裝WireGuard: apt install wireguard")
        print("2. 安裝cryptography庫: pip3 install cryptography --break-system-packages")
        print("3. 使用root權限運行: sudo python3 wireguard_server.py")
        print("4. 檢查防火牆設置")
        print("5. 確保端口未被佔用")
    finally:
        # 停止監控
        if 'server' in locals():
            server.monitoring_active = False
        
    print("\n伺服器停止")
