from wireguard_server import WireGuardServer
import json

def generate_client_connection(server: WireGuardServer, client_name: str) -> dict:
    """
    生成客戶端連線資訊
    
    Args:
        server: WireGuardServer實例
        client_name: 客戶端名稱
        
    Returns:
        包含連線資訊的字典
    """
    client_config = server.add_client(client_name)
    config_content = server.get_client_config(client_name)
    
    # 保存配置文件
    server.save_client_config(client_name)
    
    return {
        'client_name': client_name,
        'config_file_content': config_content,
        'client_ip': client_config['ip'],
        'private_key': client_config['private_key'],
        'public_key': client_config['public_key'],
        'server_endpoint': client_config['server_endpoint']
    }

def batch_generate_clients(server: WireGuardServer, client_names: list) -> dict:
    """
    批量生成客戶端配置
    
    Args:
        server: WireGuardServer實例
        client_names: 客戶端名稱列表
        
    Returns:
        包含所有客戶端配置的字典
    """
    clients_info = {}
    
    for client_name in client_names:
        try:
            client_info = generate_client_connection(server, client_name)
            clients_info[client_name] = client_info
            print(f"已生成客戶端: {client_name}")
        except Exception as e:
            print(f"生成客戶端 {client_name} 失敗: {e}")
            
    return clients_info

if __name__ == "__main__":
    # 示例使用
    server = WireGuardServer()
    
    # 生成單個客戶端
    client_info = generate_client_connection(server, "mobile_client")
    print(f"客戶端資訊: {json.dumps(client_info, indent=2, ensure_ascii=False)}")
    
    # 批量生成客戶端
    client_names = ["laptop", "phone", "tablet"]
    all_clients = batch_generate_clients(server, client_names)
    
    # 保存所有客戶端資訊到JSON文件
    with open("d:/hivemind/wireguard_configs/all_clients.json", "w", encoding="utf-8") as f:
        json.dump(all_clients, f, indent=2, ensure_ascii=False)
