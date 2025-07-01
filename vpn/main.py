from wireguard_server import WireGuardServer
from client_generator import generate_client_connection, batch_generate_clients
import json
import sys
import argparse

def main():
    parser = argparse.ArgumentParser(description="WireGuard VPN 管理程式")
    parser.add_argument("--mode", choices=["server", "generate", "batch"], 
                       required=True, help="運行模式")
    parser.add_argument("--client-name", help="客戶端名稱")
    parser.add_argument("--client-list", nargs="+", help="客戶端名稱列表")
    
    args = parser.parse_args()
    
    if args.mode == "server":
        print("啟動WireGuard伺服器...")
        server = WireGuardServer()
        
        try:
            while True:
                command = input("輸入命令 (add/remove/list/quit): ").strip().lower()
                
                if command == "add":
                    client_name = input("輸入客戶端名稱: ").strip()
                    if client_name:
                        client_info = generate_client_connection(server, client_name)
                        print(f"客戶端 {client_name} 已添加")
                        print(f"客戶端IP: {client_info['client_ip']}")
                        
                elif command == "remove":
                    client_name = input("輸入要移除的客戶端名稱: ").strip()
                    if server.remove_client(client_name):
                        print(f"客戶端 {client_name} 已移除")
                    else:
                        print(f"移除客戶端 {client_name} 失敗")
                        
                elif command == "list":
                    clients = server.list_clients()
                    print(f"當前客戶端: {clients}")
                    
                elif command == "quit":
                    break
                    
        except KeyboardInterrupt:
            print("伺服器停止")
            
    elif args.mode == "generate":
        if not args.client_name:
            print("請指定客戶端名稱")
            return
            
        server = WireGuardServer()
        client_info = generate_client_connection(server, args.client_name)
        
        print(f"客戶端配置已生成:")
        print(json.dumps(client_info, indent=2, ensure_ascii=False))
        
    elif args.mode == "batch":
        if not args.client_list:
            print("請指定客戶端列表")
            return
            
        server = WireGuardServer()
        all_clients = batch_generate_clients(server, args.client_list)
        
        print(f"批量生成完成，共 {len(all_clients)} 個客戶端")
        
        # 保存到文件
        with open("d:/hivemind/wireguard_configs/batch_clients.json", "w", encoding="utf-8") as f:
            json.dump(all_clients, f, indent=2, ensure_ascii=False)
            
        print("配置已保存到 batch_clients.json")

if __name__ == "__main__":
    main()
