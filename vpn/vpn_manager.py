import subprocess
import os
import time
import sys

class WireGuardManager:
    def __init__(self, config_path: str):
        self.config_path = config_path
        self.process = None

    def start_vpn(self):
        """啟動 VPN 連接"""
        try:
            # 假設 Go 程序編譯成 wireguard.exe
            cmd = ["./wireguard.exe", self.config_path]
            self.process = subprocess.Popen(cmd, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
            print("VPN 連接已啟動")
            return True
        except Exception as e:
            print(f"啟動 VPN 失敗: {e}")
            return False

    def stop_vpn(self):
        """停止 VPN 連接"""
        if self.process:
            self.process.terminate()
            self.process.wait()
            print("VPN 連接已停止")
            self.process = None

    def is_connected(self):
        """檢查連接狀態"""
        return self.process is not None and self.process.poll() is None

# 使用示例
if __name__ == "__main__":
    config_path = r"D:\hivemind\vpn\wg0.conf"  # 替換為實際配置路徑

    manager = WireGuardManager(config_path)

    # 啟動 VPN
    if manager.start_vpn():
        print("VPN 已連接")

        # 保持運行
        try:
            while True:
                time.sleep(1)
        except KeyboardInterrupt:
            manager.stop_vpn()
    else:
        print("無法啟動 VPN")