import subprocess
import os

# 定義 WSL 命令執行函數
def run_wsl_command(command):
    # 使用 sudo 执行命令，确保路径正确
    result = subprocess.run(['wsl', '-d', 'ubuntu', '--', 'sudo', 'bash', '-c', command], capture_output=True, text=True)
    if result.returncode != 0:
        raise Exception(f"命令執行失敗: {result.stderr}")
    return result.stdout.strip()

# 生成 WireGuard 公私鑰對
def generate_keys():
    private_key = run_wsl_command("wg genkey")
    public_key = run_wsl_command(f"echo {private_key} | wg pubkey")
    return private_key, public_key

# 生成服務器配置文件
def generate_server_config(server_private_key, server_port=51820):
    config = f"""
[Interface]
PrivateKey = {server_private_key}
Address = 10.0.0.1/24
ListenPort = {server_port}
PostUp = iptables -A FORWARD -i wg0 -j ACCEPT; iptables -t nat -A POSTROUTING -o eth0 -j MASQUERADE
PostDown = iptables -D FORWARD -i wg0 -j ACCEPT; iptables -t nat -D POSTROUTING -o eth0 -j MASQUERADE
"""
    # 使用绝对路径确保文件位置正确
    local_path = os.path.expanduser(f"~/wg0.conf")
    with open(local_path, "w") as f:
        f.write(config)

    # 將配置文件移動到 WSL 的 /etc/wireguard/
    wsl_path = f"/mnt/c/Users/{os.getlogin()}/wg0.conf"
    run_wsl_command(f"mv {wsl_path} /etc/wireguard/wg0.conf")
    run_wsl_command("chmod 600 /etc/wireguard/wg0.conf")

# 生成客戶端配置文件
def generate_client_config(server_public_key, client_private_key, server_endpoint, client_ip="10.0.0.2/24"):
    config = f"""
[Interface]
PrivateKey = {client_private_key}
Address = {client_ip}
DNS = 8.8.8.8

[Peer]
PublicKey = {server_public_key}
Endpoint = {server_endpoint}
AllowedIPs = 0.0.0.0/0, ::/0
PersistentKeepalive = 25
"""
    with open("client.conf", "w") as f:
        f.write(config)
    print("客戶端配置文件已生成：client.conf")

# 啟動 WireGuard 服務器
def start_wireguard():
    run_wsl_command("wg-quick up wg0")
    print("WireGuard 服務器已啟動！")

# 主程序
def main():
    # 生成服務器密鑰對
    server_private_key, server_public_key = generate_keys()
    # 生成客戶端密鑰對
    client_private_key, client_public_key = generate_keys()

    # 配置參數
    server_port = 51820
    server_endpoint = "163.30.123.6:51820"  # 替換為你的服務器公網 IP

    # 生成服務器配置文件並移動到 WSL
    generate_server_config(server_private_key, server_port)

    # 生成客戶端配置文件（保存到本地）
    generate_client_config(server_public_key, client_private_key, server_endpoint)

    # 啟動 WireGuard
    start_wireguard()

    # 輸出客戶端公鑰，供服務器添加
    print(f"客戶端公鑰（請手動添加到服務器配置）: {client_public_key}")

if __name__ == "__main__":
    main()
