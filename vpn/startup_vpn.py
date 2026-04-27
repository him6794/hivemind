import os
import sys
import time
from vpn_manager import WireGuardManager

def main():
    # WireGuard 配置路徑
    config_path = os.path.join(os.path.dirname(__file__), "wireguard.conf")

    # 如果配置文件不存在，創建一個示例
    if not os.path.exists(config_path):
        print("警告: 找不到 WireGuard 配置文件，將創建示例配置")
        create_example_config(config_path)

    # 創建 VPN 管理器
    manager = WireGuardManager(config_path)

    # 啟動 VPN
    if manager.start_vpn():
        print("開機 VPN 連接已啟動")

        # 保持運行
        try:
            while True:
                if not manager.is_connected():
                    print("VPN 連接斷開，嘗試重新連接...")
                    time.sleep(5)
                    manager.start_vpn()
                time.sleep(10)
        except KeyboardInterrupt:
            print("收到停止信號，正在關閉 VPN...")
            manager.stop_vpn()
    else:
        print("無法啟動開機 VPN 連接")
        sys.exit(1)

def create_example_config(config_path):
    """創建示例 WireGuard 配置"""
    example_config = """[Interface]
PrivateKey = YOUR_PRIVATE_KEY_HERE
Address = 10.0.0.2/24
DNS = 8.8.8.8

[Peer]
PublicKey = SERVER_PUBLIC_KEY_HERE
Endpoint = server.example.com:51820
AllowedIPs = 0.0.0.0/0
PersistentKeepalive = 25
"""
    with open(config_path, 'w') as f:
        f.write(example_config)
    print(f"已創建示例配置: {config_path}")
    print("請編輯配置文件並填入正確的金鑰和端點")

if __name__ == "__main__":
    main()