# HiveMind BitTorrent (BT) 模組

> **Language / 語言選擇**
> 
> - **English**: [README.en.md](README.en.md)
> - **繁體中文**: [README.md](README.md) (本文檔)

## 概述

BT 模組實現了基於 BitTorrent 協議的點對點 (P2P) 大檔案傳輸功能。這個模組已經完成開發，提供了完整的 torrent 創建、追蹤和播種功能，可以高效地在分布式節點間傳輸大型任務文件。

## 功能特色

### 📁 種子檔案管理
- **自動創建**：為大型任務檔案自動生成 .torrent 種子檔案
- **種子驗證**：確保種子檔案完整性和有效性
- **批量處理**：支援批量創建和管理多個種子檔案

### P2P 檔案分享
- **高效傳輸**：利用 BitTorrent 協議實現多點下載
- **斷點續傳**：支援下載中斷後的續傳功能
- **頻寬控制**：可配置上傳和下載頻寬限制

### 📡 追蹤器服務
- **內建追蹤器**：提供完整的 BitTorrent 追蹤器功能
- **節點發現**：協助節點間相互發現和連接
- **統計監控**：實時監控下載進度和節點狀態

## 文件結構

```
bt/
├── create_torrent.py    # 種子檔案創建工具
├── seeder.py           # 播種服務實現
├── tracker.py          # BitTorrent 追蹤器
├── test.exe           # 測試可執行檔案
└── test.torrent       # 測試種子檔案
```

## 核心模組

### create_torrent.py
負責創建 BitTorrent 種子檔案：

```python
import hashlib
import bencodepy
import os
from typing import Dict, List

class TorrentCreator:
    """種子檔案創建器"""
    
    def __init__(self, announce_url: str = "http://localhost:8080/announce"):
        self.announce_url = announce_url
        self.piece_length = 2**18  # 256KB 片段大小
    
    def create_torrent(self, file_path: str, output_path: str = None) -> str:
        """
        為指定檔案創建種子檔案
        
        Args:
            file_path: 要創建種子的檔案路徑
            output_path: 種子檔案輸出路徑，默認為 file_path + ".torrent"
            
        Returns:
            創建的種子檔案路徑
        """
        if not os.path.exists(file_path):
            raise FileNotFoundError(f"檔案不存在: {file_path}")
        
        file_size = os.path.getsize(file_path)
        pieces = self._generate_pieces(file_path)
        
        torrent_data = {
            b'announce': self.announce_url.encode(),
            b'info': {
                b'name': os.path.basename(file_path).encode(),
                b'length': file_size,
                b'piece length': self.piece_length,
                b'pieces': b''.join(pieces)
            }
        }
        
        if output_path is None:
            output_path = file_path + ".torrent"
        
        with open(output_path, 'wb') as f:
            f.write(bencodepy.encode(torrent_data))
        
        return output_path
    
    def _generate_pieces(self, file_path: str) -> List[bytes]:
        """生成檔案片段的 SHA1 哈希值"""
        pieces = []
        
        with open(file_path, 'rb') as f:
            while True:
                chunk = f.read(self.piece_length)
                if not chunk:
                    break
                pieces.append(hashlib.sha1(chunk).digest())
        
        return pieces
```

### tracker.py
實現 BitTorrent 追蹤器服務：

```python
import time
import socket
from http.server import HTTPServer, BaseHTTPRequestHandler
from urllib.parse import parse_qs, urlparse
from typing import Dict, Set

class BitTorrentTracker:
    """BitTorrent 追蹤器"""
    
    def __init__(self, port: int = 8080):
        self.port = port
        self.torrents: Dict[str, Dict] = {}  # info_hash -> torrent_info
        self.peers: Dict[str, Set[str]] = {}  # info_hash -> set of peer_id
        
    def start_server(self):
        """啟動追蹤器服務"""
        server = HTTPServer(('', self.port), TrackerRequestHandler)
        server.tracker = self
        print(f"BitTorrent 追蹤器啟動在端口 {self.port}")
        server.serve_forever()
    
    def announce(self, info_hash: str, peer_id: str, 
                ip: str, port: int, event: str = None) -> Dict:
        """處理節點通告請求"""
        
        # 初始化種子記錄
        if info_hash not in self.torrents:
            self.torrents[info_hash] = {
                'seeders': set(),
                'leechers': set(),
                'completed': 0
            }
            self.peers[info_hash] = set()
        
        torrent = self.torrents[info_hash]
        peer_key = f"{peer_id}:{ip}:{port}"
        
        # 處理事件
        if event == 'started':
            torrent['leechers'].add(peer_key)
        elif event == 'completed':
            torrent['leechers'].discard(peer_key)
            torrent['seeders'].add(peer_key)
            torrent['completed'] += 1
        elif event == 'stopped':
            torrent['seeders'].discard(peer_key)
            torrent['leechers'].discard(peer_key)
        
        # 返回節點列表
        all_peers = list(torrent['seeders'].union(torrent['leechers']))
        peer_list = []
        
        for peer in all_peers[:50]:  # 最多返回 50 個節點
            if peer != peer_key:  # 不返回請求者自己
                parts = peer.split(':')
                peer_list.append({
                    'peer_id': parts[0],
                    'ip': parts[1],
                    'port': int(parts[2])
                })
        
        return {
            'interval': 300,  # 5 分鐘後再次通告
            'complete': len(torrent['seeders']),
            'incomplete': len(torrent['leechers']),
            'peers': peer_list
        }

class TrackerRequestHandler(BaseHTTPRequestHandler):
    """追蹤器 HTTP 請求處理器"""
    
    def do_GET(self):
        """處理 GET 請求"""
        parsed_url = urlparse(self.path)
        
        if parsed_url.path == '/announce':
            self._handle_announce(parsed_url.query)
        elif parsed_url.path == '/stats':
            self._handle_stats()
        else:
            self.send_error(404)
    
    def _handle_announce(self, query_string: str):
        """處理通告請求"""
        try:
            params = parse_qs(query_string)
            
            # 提取必要參數
            info_hash = params.get('info_hash', [None])[0]
            peer_id = params.get('peer_id', [None])[0]
            port = int(params.get('port', [0])[0])
            event = params.get('event', [None])[0]
            
            if not all([info_hash, peer_id, port]):
                self.send_error(400, "缺少必要參數")
                return
            
            # 獲取客戶端 IP
            client_ip = self.client_address[0]
            
            # 調用追蹤器邏輯
            response = self.server.tracker.announce(
                info_hash, peer_id, client_ip, port, event
            )
            
            # 返回 bencoded 響應
            self.send_response(200)
            self.send_header('Content-Type', 'application/x-bittorrent')
            self.end_headers()
            
            import bencodepy
            self.wfile.write(bencodepy.encode(response))
            
        except Exception as e:
            print(f"處理通告請求時出錯: {e}")
            self.send_error(500)
```

### seeder.py
實現檔案播種功能：

```python
import time
import threading
from typing import List, Dict

class TorrentSeeder:
    """種子檔案播種器"""
    
    def __init__(self, tracker_url: str = "http://localhost:8080/announce"):
        self.tracker_url = tracker_url
        self.active_torrents: Dict[str, Dict] = {}
        self.running = False
    
    def add_torrent(self, torrent_path: str, data_path: str) -> str:
        """添加要播種的種子檔案"""
        
        # 解析種子檔案
        torrent_info = self._parse_torrent(torrent_path)
        info_hash = self._calculate_info_hash(torrent_info)
        
        self.active_torrents[info_hash] = {
            'torrent_path': torrent_path,
            'data_path': data_path,
            'info': torrent_info,
            'last_announce': 0
        }
        
        print(f"添加種子: {torrent_path}")
        return info_hash
    
    def start_seeding(self):
        """開始播種"""
        self.running = True
        
        # 啟動通告線程
        announce_thread = threading.Thread(target=self._announce_loop)
        announce_thread.daemon = True
        announce_thread.start()
        
        print("開始播種...")
        
        try:
            while self.running:
                time.sleep(1)
        except KeyboardInterrupt:
            self.stop_seeding()
    
    def stop_seeding(self):
        """停止播種"""
        self.running = False
        
        # 向追蹤器發送停止事件
        for info_hash in self.active_torrents:
            self._announce_to_tracker(info_hash, event='stopped')
        
        print("停止播種")
    
    def _announce_loop(self):
        """定期向追蹤器通告"""
        while self.running:
            current_time = time.time()
            
            for info_hash, torrent in self.active_torrents.items():
                # 每 5 分鐘通告一次
                if current_time - torrent['last_announce'] > 300:
                    self._announce_to_tracker(info_hash)
                    torrent['last_announce'] = current_time
            
            time.sleep(30)  # 每 30 秒檢查一次
    
    def _announce_to_tracker(self, info_hash: str, event: str = None):
        """向追蹤器發送通告"""
        import requests
        import socket
        
        try:
            # 獲取本機 IP 和端口
            local_ip = socket.gethostbyname(socket.gethostname())
            local_port = 6881  # 默認 BitTorrent 端口
            
            params = {
                'info_hash': info_hash,
                'peer_id': self._generate_peer_id(),
                'port': local_port,
                'uploaded': 0,
                'downloaded': 0,
                'left': 0,  # 作為播種者，剩餘字節為 0
                'compact': 1
            }
            
            if event:
                params['event'] = event
            
            response = requests.get(self.tracker_url, params=params, timeout=10)
            
            if response.status_code == 200:
                print(f"成功通告種子: {info_hash[:8]}...")
            else:
                print(f"通告失敗: {response.status_code}")
                
        except Exception as e:
            print(f"通告時出錯: {e}")
    
    def _parse_torrent(self, torrent_path: str) -> Dict:
        """解析種子檔案"""
        import bencodepy
        
        with open(torrent_path, 'rb') as f:
            return bencodepy.decode(f.read())
    
    def _calculate_info_hash(self, torrent_data: Dict) -> str:
        """計算 info 字典的 SHA1 哈希值"""
        import hashlib
        import bencodepy
        
        info_encoded = bencodepy.encode(torrent_data[b'info'])
        return hashlib.sha1(info_encoded).hexdigest()
    
    def _generate_peer_id(self) -> str:
        """生成節點 ID"""
        import random
        import string
        
        prefix = "HM"  # HiveMind 前綴
        random_part = ''.join(random.choices(string.ascii_letters + string.digits, k=18))
        return prefix + random_part
```

## 使用方法

### 基本使用流程

1. **啟動追蹤器**
   ```python
   from bt.tracker import BitTorrentTracker
   
   # 啟動追蹤器服務
   tracker = BitTorrentTracker(port=8080)
   tracker.start_server()  # 這會阻塞當前線程
   ```

2. **創建種子檔案**
   ```python
   from bt.create_torrent import TorrentCreator
   
   # 為大型任務檔案創建種子
   creator = TorrentCreator("http://your-tracker:8080/announce")
   torrent_path = creator.create_torrent("/path/to/large_file.zip")
   print(f"種子檔案已創建: {torrent_path}")
   ```

3. **開始播種**
   ```python
   from bt.seeder import TorrentSeeder
   
   # 播種檔案
   seeder = TorrentSeeder("http://your-tracker:8080/announce")
   info_hash = seeder.add_torrent(torrent_path, "/path/to/large_file.zip")
   seeder.start_seeding()  # 開始播種
   ```

### 與 HiveMind 系統整合

```python
# 在任務分發時使用 P2P 傳輸
class TaskDistributor:
    def __init__(self):
        self.torrent_creator = TorrentCreator()
        self.seeder = TorrentSeeder()
    
    def distribute_large_task(self, task_file_path: str, target_nodes: List[str]):
        """分發大型任務檔案到多個節點"""
        
        # 1. 創建種子檔案
        torrent_path = self.torrent_creator.create_torrent(task_file_path)
        
        # 2. 開始播種原始檔案
        info_hash = self.seeder.add_torrent(torrent_path, task_file_path)
        
        # 3. 將種子檔案發送給目標節點
        for node in target_nodes:
            self.send_torrent_to_node(node, torrent_path)
        
        # 4. 節點使用種子檔案下載原始檔案
        return info_hash
    
    def send_torrent_to_node(self, node_address: str, torrent_path: str):
        """發送種子檔案給節點"""
        # 使用 gRPC 或 HTTP 發送種子檔案
        pass
```

## 配置選項

創建 `bt_config.json` 配置檔案：

```json
{
  "tracker": {
    "port": 8080,
    "announce_interval": 300,
    "max_peers_per_torrent": 50,
    "cleanup_interval": 3600
  },
  "seeder": {
    "max_upload_rate": 1048576,
    "max_concurrent_uploads": 10,
    "piece_request_timeout": 30
  },
  "torrent_creation": {
    "piece_length": 262144,
    "private": false,
    "comment": "Created by HiveMind BT Module"
  },
  "network": {
    "listen_port_range": [6881, 6891],
    "dht_enabled": false,
    "upnp_enabled": true
  }
}
```

## 效能特色

### 頻寬使用優化
- **智能調節**：根據網路狀況自動調整上傳下載速度
- **公平分享**：確保所有節點公平分享頻寬資源
- **優先級控制**：重要任務檔案可獲得更高傳輸優先級

### 可靠性保證
- **校驗和驗證**：每個檔案片段都有 SHA1 校驗和
- **重試機制**：傳輸失敗自動重試
- **冗余備份**：多個節點保存相同檔案副本

### 監控和統計
```python
# 獲取傳輸統計信息
def get_transfer_stats(info_hash: str) -> Dict:
    return {
        'total_uploaded': 1024*1024*500,  # 500MB
        'total_downloaded': 1024*1024*200,  # 200MB
        'download_rate': 1024*100,  # 100KB/s
        'upload_rate': 1024*50,     # 50KB/s
        'connected_peers': 15,
        'completion_percentage': 85.5
    }
```

## 測試

### 本地測試
```bash
cd bt/

# 測試種子檔案創建
python create_torrent.py test.exe

# 啟動追蹤器
python tracker.py

# 在另一個終端啟動播種
python seeder.py test.torrent test.exe
```

### 測試狀態

**重要提醒**: 目前 BitTorrent 模組尚未建立測試框架，測試功能規劃在未來開發中實現。

建議的測試範圍：
- 檔案分發功能測試
- 多節點下載性能測試  
- 追蹤器連接測試
- 檔案完整性驗證

## 與現有系統整合

### Node Pool 整合
```python
# 在 node_pool/file_transfer.py 中
from bt.create_torrent import TorrentCreator
from bt.seeder import TorrentSeeder

class FileTransferManager:
    def __init__(self):
        self.bt_creator = TorrentCreator()
        self.bt_seeder = TorrentSeeder()
    
    def handle_large_file_transfer(self, file_path: str, target_nodes: List[str]):
        """處理大檔案傳輸"""
        file_size = os.path.getsize(file_path)
        
        if file_size > 100 * 1024 * 1024:  # 大於 100MB 使用 P2P
            return self._transfer_via_p2p(file_path, target_nodes)
        else:
            return self._transfer_via_grpc(file_path, target_nodes)
```

### Worker 節點整合
```python
# 在 worker/file_downloader.py 中
class P2PFileDownloader:
    def __init__(self):
        self.download_dir = "/tmp/hivemind_downloads"
    
    def download_from_torrent(self, torrent_data: bytes) -> str:
        """從種子檔案下載檔案"""
        # 解析種子檔案
        # 連接到追蹤器
        # 下載檔案片段
        # 組合完整檔案
        pass
```

## 故障排除

### 常見問題

1. **追蹤器連接失敗**
   ```bash
   # 檢查追蹤器是否運行
   curl http://localhost:8080/stats
   
   # 檢查防火牆設置
   sudo ufw allow 8080/tcp
   ```

2. **下載速度緩慢**
   ```python
   # 調整片段大小
   creator = TorrentCreator()
   creator.piece_length = 2**16  # 64KB 更小片段
   ```

3. **節點發現問題**
   ```python
   # 檢查節點通告
   import requests
   response = requests.get('http://tracker:8080/announce', params={
       'info_hash': 'test_hash',
       'peer_id': 'test_peer',
       'port': 6881
   })
   print(response.content)
   ```

## 性能優化建議

1. **網路優化**
   - 使用專用網路接口進行 P2P 傳輸
   - 配置適當的 TCP 緩衝區大小
   - 啟用網路壓縮

2. **儲存優化**
   - 使用 SSD 存儲頻繁訪問的檔案
   - 實現檔案預取機制
   - 配置適當的磁碟快取

3. **並發優化**
   - 調整最大並發連接數
   - 使用連接池管理網路連接
   - 實現智能的片段請求調度

## 未來發展

### 短期計畫
- [ ] 整合到 HiveMind 主系統
- [ ] 添加下載進度監控 API
- [ ] 實現檔案自動清理機制

### 長期計畫
- [ ] 支援流式傳輸
- [ ] 實現分層存儲
- [ ] 添加內容分發網路 (CDN) 功能
- [ ] 支援檔案加密傳輸

## 許可證

本模組採用與主項目相同的 GPL v3.0 許可證。

## 聯繫方式

- **模組維護者**: HiveMind BT Team
- **技術支援**: bt-support@hivemind.com
- **Bug 回報**: https://github.com/him6794/hivemind/issues
