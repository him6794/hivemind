import subprocess
import socket
import threading
import time
import json
from pathlib import Path
import sys
import platform
import shutil
import os
import logging
from datetime import datetime
import re

class WireGuardClientFixed:
    def __init__(self, config_file_path: str):
        self.config_file = Path(config_file_path)
        self.interface_name = "wg_client"
        self.proxy_ports = [50001, 50002, 50003]
        self.is_connected = False
        self.is_windows = platform.system() == "Windows"
        
        # 尋找WireGuard工具
        self.wg_path, self.wg_quick_path = self._find_wireguard_tools()
        
        self.vpn_interface_name = None
        self.actual_interface_name = None
        
        # 設置日誌
        self._setup_logging()
        
    def _setup_logging(self):
        """設置客戶端日誌"""
        log_dir = Path("d:/hivemind/vpn/logs")
        log_dir.mkdir(parents=True, exist_ok=True)
        
        log_file = log_dir / f"client_fixed_{datetime.now().strftime('%Y%m%d')}.log"
        
        logging.basicConfig(
            level=logging.INFO,
            format='%(asctime)s - CLIENT - %(levelname)s - %(message)s',
            handlers=[
                logging.FileHandler(log_file, encoding='utf-8'),
                logging.StreamHandler()
            ]
        )
        
        self.logger = logging.getLogger(f"{__name__}_client_fixed")
        self.logger.info("WireGuard客戶端啟動")
        
    def _find_wireguard_tools(self):
        """尋找WireGuard工具路徑"""
        wg_candidates = []
        wg_quick_candidates = []
        
        if self.is_windows:
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
            wg_candidates = ["wg"]
            wg_quick_candidates = ["wg-quick"]
        
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
        
        return wg_path, wg_quick_path
        
    def _get_real_interface_name(self):
        """獲取真實的WireGuard介面名稱"""
        try:
            if self.wg_path:
                result = subprocess.run([self.wg_path, 'show', 'interfaces'], 
                                      capture_output=True, text=True)
                if result.returncode == 0 and result.stdout.strip():
                    interfaces = result.stdout.strip().split('\n')
                    for interface in interfaces:
                        interface = interface.strip()
                        if interface:
                            print(f"發現WireGuard介面: {interface}")
                            self.logger.info(f"發現WireGuard介面: {interface}")
                            self.actual_interface_name = interface
                            return interface
                            
            # 從配置文件名推測
            config_name = self.config_file.stem
            print(f"使用配置文件名作為介面: {config_name}")
            self.actual_interface_name = config_name
            return config_name
            
        except Exception as e:
            print(f"獲取介面名稱失敗: {e}")
            self.logger.error(f"獲取介面名稱失敗: {e}")
            return "test"
            
    def connect(self):
        """連接到WireGuard VPN"""
        if self.is_windows:
            return self._connect_windows()
        else:
            return self._connect_linux()
            
    def _connect_windows(self):
        """Windows環境連接VPN"""
        try:
            print(f"正在連接WireGuard配置: {self.config_file}")
            
            if not self.config_file.exists():
                print(f"配置文件不存在: {self.config_file}")
                return False
            
            config_content = self.config_file.read_text()
            
            if "YOUR_SERVER_IP" in config_content:
                print("錯誤：配置文件中仍包含未設定的伺服器IP")
                return False
            
            print("配置文件內容:")
            print("=" * 50)
            print(config_content)
            print("=" * 50)
            
            print("\nWindows環境連接步驟:")
            print("1. 開啟WireGuard GUI應用程式")
            print("2. 點擊 '添加隧道' -> '從文件導入'")
            print(f"3. 選擇配置文件: {self.config_file.absolute()}")
            print("4. 點擊 '激活' 按鈕啟動連接")
            print("5. 確認連接狀態顯示為 '已激活'")
            
            self._check_wireguard_gui()
            
            response = input("\n請完成上述步驟後輸入'y'確認連接成功: ")
            if response.lower() in ['y', 'yes']:
                self.is_connected = True
                print("✓ VPN連接已確認")
                
                # 獲取實際介面名稱
                self.vpn_interface_name = self._get_real_interface_name()
                
                # 檢查連接狀態
                if self._check_connection_status():
                    print("✓ VPN連接驗證成功")
                    return True
                else:
                    print("⚠ VPN連接可能有問題，但繼續運行")
                    return True
            else:
                print("VPN連接取消")
                return False
                
        except Exception as e:
            print(f"Windows VPN連接失敗: {e}")
            return False
            
    def _check_wireguard_gui(self):
        """檢查WireGuard GUI是否運行"""
        try:
            result = subprocess.run(['tasklist', '/FI', 'IMAGENAME eq wireguard.exe'], 
                                  capture_output=True, text=True, errors='ignore')
            
            if 'wireguard.exe' in result.stdout:
                print("✓ 檢測到WireGuard GUI正在運行")
                self.logger.info("WireGuard GUI正在運行")
            else:
                print("⚠ 未檢測到WireGuard GUI，請先啟動應用程式")
                self.logger.warning("WireGuard GUI未運行")
                input("啟動WireGuard GUI後按Enter繼續...")
                
        except Exception as e:
            print(f"檢查WireGuard GUI狀態失敗: {e}")
            
    def _check_connection_status(self):
        """檢查連接狀態"""
        try:
            if self.wg_path and self.actual_interface_name:
                result = subprocess.run([self.wg_path, 'show', self.actual_interface_name], 
                                      capture_output=True, text=True)
                
                if result.returncode == 0 and result.stdout:
                    print(f"WireGuard {self.actual_interface_name} 狀態:")
                    print(result.stdout)
                    self.logger.info(f"WireGuard狀態: {result.stdout}")
                    
                    # 檢查是否有peer連接
                    if 'peer:' in result.stdout.lower() or 'handshake:' in result.stdout.lower():
                        print("✓ 檢測到peer連接")
                        
                        # 測試VPN網段連接
                        return self._test_vpn_connectivity()
                    else:
                        print("⚠ 未檢測到活躍的peer連接")
                        return False
                        
        except Exception as e:
            print(f"檢查連接狀態失敗: {e}")
            self.logger.error(f"檢查連接狀態失敗: {e}")
            
        return False
        
    def _test_vpn_connectivity(self):
        """測試VPN連接性"""
        print("測試VPN連接性...")
        
        # 測試VPN網段內的連接
        test_targets = [
            ('10.0.0.1', 'VPN閘道'),
            ('10.0.0.2', 'VPN網段其他節點')
        ]
        
        success_count = 0
        for ip, description in test_targets:
            print(f"測試連接到 {description} ({ip})...")
            result = subprocess.run(['ping', '-n', '1', '-w', '3000', ip], 
                                  capture_output=True, text=True)
            
            if result.returncode == 0:
                print(f"✓ {description} 連接成功")
                self.logger.info(f"VPN連接測試成功: {description} ({ip})")
                success_count += 1
            else:
                print(f"✗ {description} 連接失敗")
                self.logger.warning(f"VPN連接測試失敗: {description} ({ip})")
                
        return success_count > 0
        
    def setup_port_forwarding(self):
        """設置端口轉發規則，只代理指定端口"""
        if not self.is_connected:
            print("請先連接VPN")
            return False
            
        print("設置端口代理服務...")
        
        # 啟動代理服務器
        for port in self.proxy_ports:
            self._start_proxy_server(port)
            
        return True
        
    def _start_proxy_server(self, local_port: int):
        """啟動代理服務器"""
        def proxy_handler():
            try:
                server_socket = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
                server_socket.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
                
                # 綁定到本地端口
                proxy_port = local_port + 10000
                server_socket.bind(('127.0.0.1', proxy_port))
                server_socket.listen(5)
                print(f"代理服務器已啟動: 127.0.0.1:{proxy_port} -> VPN網段:{local_port}")
                
                while self.is_connected:
                    try:
                        server_socket.settimeout(5)
                        client_socket, address = server_socket.accept()
                        print(f"代理連接來自: {address}")
                        
                        # 處理代理連接
                        proxy_thread = threading.Thread(
                            target=self._handle_proxy_connection,
                            args=(client_socket, local_port)
                        )
                        proxy_thread.daemon = True
                        proxy_thread.start()
                        
                    except socket.timeout:
                        continue
                    except Exception as e:
                        if self.is_connected:
                            print(f"代理服務器錯誤: {e}")
                        break
                        
            except Exception as e:
                print(f"代理服務器 {local_port} 啟動失敗: {e}")
            finally:
                try:
                    server_socket.close()
                except:
                    pass
                    
        proxy_thread = threading.Thread(target=proxy_handler)
        proxy_thread.daemon = True
        proxy_thread.start()
        
    def _handle_proxy_connection(self, client_socket, target_port):
        """處理代理連接"""
        try:
            # 嘗試連接到VPN網段的服務
            target_ips = ['10.0.0.1', '10.0.0.2', '10.0.0.3']
            
            for target_ip in target_ips:
                try:
                    vpn_socket = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
                    vpn_socket.settimeout(5)
                    vpn_socket.connect((target_ip, target_port))
                    print(f"代理連接成功: {target_ip}:{target_port}")
                    
                    # 啟動雙向數據轉發
                    self._forward_data(client_socket, vpn_socket)
                    return
                    
                except Exception:
                    vpn_socket.close()
                    continue
                    
            # 如果無法連接到VPN服務，返回測試響應
            response = f"HTTP/1.1 200 OK\r\nContent-Length: 50\r\n\r\nVPN代理服務運行中，但目標端口 {target_port} 無服務"
            client_socket.send(response.encode())
            
        except Exception as e:
            print(f"代理連接處理錯誤: {e}")
        finally:
            try:
                client_socket.close()
            except:
                pass
                
    def _forward_data(self, client_socket, vpn_socket):
        """雙向數據轉發"""
        def forward(source, destination):
            try:
                while True:
                    data = source.recv(4096)
                    if not data:
                        break
                    destination.send(data)
            except:
                pass
            finally:
                try:
                    source.close()
                    destination.close()
                except:
                    pass
                    
        # 啟動雙向轉發線程
        client_to_vpn = threading.Thread(target=forward, args=(client_socket, vpn_socket))
        vpn_to_client = threading.Thread(target=forward, args=(vpn_socket, client_socket))
        
        client_to_vpn.daemon = True
        vpn_to_client.daemon = True
        
        client_to_vpn.start()
        vpn_to_client.start()
        
        # 等待任一方向結束
        client_to_vpn.join(timeout=300)  # 5分鐘超時
        vpn_to_client.join(timeout=300)
        
    def test_proxy_ports(self):
        """測試代理端口"""
        print("測試代理端口...")
        for port in self.proxy_ports:
            proxy_port = port + 10000
            try:
                sock = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
                sock.settimeout(3)
                
                result = sock.connect_ex(('127.0.0.1', proxy_port))
                
                if result == 0:
                    print(f"代理端口 {proxy_port} (->VPN:{port}): 可連接")
                    # 發送測試請求
                    sock.send(b'GET / HTTP/1.1\r\nHost: test\r\n\r\n')
                    response = sock.recv(1024)
                    print(f"  響應: {response[:50].decode(errors='ignore')}...")
                else:
                    print(f"代理端口 {proxy_port} (->VPN:{port}): 無法連接")
                    
                sock.close()
                
            except Exception as e:
                print(f"測試代理端口 {proxy_port} 失敗: {e}")
                
    def disconnect(self):
        """斷開VPN連接"""
        self.is_connected = False
        print("請在WireGuard GUI中手動斷開連接")
        return True

def create_test_service(port: int):
    """創建測試服務"""
    def handle_client(client_socket, address):
        try:
            data = client_socket.recv(1024)
            response = f"HTTP/1.1 200 OK\r\nContent-Length: 25\r\n\r\nLocal port {port} working!"
            client_socket.send(response.encode())
        except Exception as e:
            print(f"處理客戶端請求失敗: {e}")
        finally:
            client_socket.close()
            
    def start_server():
        server_socket = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        server_socket.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
        
        try:
            server_socket.bind(('127.0.0.1', port))
            server_socket.listen(5)
            print(f"本地測試服務已啟動在端口 {port}")
            
            while True:
                client_socket, address = server_socket.accept()
                client_thread = threading.Thread(
                    target=handle_client, 
                    args=(client_socket, address)
                )
                client_thread.daemon = True
                client_thread.start()
                
        except Exception as e:
            print(f"端口 {port} 服務啟動失敗: {e}")
        finally:
            server_socket.close()
            
    server_thread = threading.Thread(target=start_server)
    server_thread.daemon = True
    server_thread.start()

if __name__ == "__main__":
    if len(sys.argv) != 2:
        print("使用方法: python client_test_fixed.py <config_file_path>")
        print("示例: python client_test_fixed.py D:\\hivemind\\wireguard_configs\\test.conf")
        sys.exit(1)
        
    config_file = sys.argv[1]
    
    if not Path(config_file).exists():
        print(f"配置文件不存在: {config_file}")
        sys.exit(1)
    
    # 創建本地測試服務
    for port in [50001, 50002, 50003]:
        create_test_service(port)
        
    time.sleep(1)
    
    client = WireGuardClientFixed(config_file)
    
    print(f"平台: {platform.system()}")
    print(f"WireGuard工具: {client.wg_path}")
    print(f"配置文件: {config_file}")
    
    try:
        if client.connect():
            client.setup_port_forwarding()
            
            print("\n代理服務已啟動！")
            print("代理端口對應:")
            for port in client.proxy_ports:
                print(f"  本地 127.0.0.1:{port + 10000} -> VPN網段 10.0.0.x:{port}")
            
            client.test_proxy_ports()
            
            print("\n客戶端運行中，按Ctrl+C退出")
            print("可用命令:")
            print("- test: 測試代理端口")
            print("- status: 顯示狀態")
            
            while True:
                try:
                    command = input("輸入命令 (或直接按Enter跳過): ").strip().lower()
                    if command == "test":
                        client.test_proxy_ports()
                    elif command == "status":
                        client._check_connection_status()
                    elif command == "":
                        time.sleep(1)
                    else:
                        print("未知命令")
                except EOFError:
                    break
                except KeyboardInterrupt:
                    break
                    
    except KeyboardInterrupt:
        print("正在停止客戶端...")
        client.disconnect()
        print("客戶端已停止")
