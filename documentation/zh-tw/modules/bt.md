# BT Module 模組文檔

## 📋 概述

BT (BitTorrent) Module 是 HiveMind 分散式計算平台的點對點文件傳輸模組，負責大型數據集和模型文件的高效分發與共享。基於 BitTorrent 協議實現去中心化文件傳輸。

## 🏗️ 系統架構

```
┌─────────────────────┐
│    BT Module        │
├─────────────────────┤
│ • Torrent Creator   │
│ • Seeder Service    │
│ • Tracker Server    │
│ • Download Manager  │
│ • Peer Discovery    │
└─────────────────────┘
        │
        ├─ P2P Network
        ├─ DHT (Distributed Hash Table)
        ├─ Tracker Network
        └─ Local Storage
```

## 🔧 核心組件

### 1. Torrent Creator (`create_torrent.py`)
- **功能**: 創建和管理 torrent 文件
- **狀態**: 基礎實現完成
- **用途**: 將大型文件打包為 torrent 進行分發

**主要功能**:
```python
import hashlib
import bencodepy
import os
from datetime import datetime

class TorrentCreator:
    def __init__(self, announce_url="http://localhost:8080/announce"):
        self.announce_url = announce_url
        self.piece_length = 256 * 1024  # 256KB pieces
    
    def create_torrent(self, file_path, output_path=None):
        """創建 torrent 文件"""
        if not os.path.exists(file_path):
            raise FileNotFoundError(f"文件不存在: {file_path}")
        
        # 計算文件塊資訊
        pieces = self._calculate_pieces(file_path)
        file_size = os.path.getsize(file_path)
        
        # 構建 torrent 字典
        torrent_data = {
            b'announce': self.announce_url.encode(),
            b'info': {
                b'name': os.path.basename(file_path).encode(),
                b'length': file_size,
                b'piece length': self.piece_length,
                b'pieces': pieces
            },
            b'creation date': int(datetime.now().timestamp()),
            b'created by': b'HiveMind BT Module v1.0'
        }
        
        # 編碼並保存
        encoded_data = bencodepy.encode(torrent_data)
        
        if output_path is None:
            output_path = file_path + '.torrent'
        
        with open(output_path, 'wb') as f:
            f.write(encoded_data)
        
        return output_path
    
    def _calculate_pieces(self, file_path):
        """計算文件塊 SHA1 哈希值"""
        pieces = b''
        
        with open(file_path, 'rb') as f:
            while True:
                piece = f.read(self.piece_length)
                if not piece:
                    break
                
                piece_hash = hashlib.sha1(piece).digest()
                pieces += piece_hash
        
        return pieces
```

### 2. Seeder Service (`seeder.py`)
- **功能**: BitTorrent 種子服務器
- **狀態**: 核心功能實現
- **協議**: BitTorrent Protocol

**實現範例**:
```python
import socket
import threading
import struct
import time
from urllib.parse import unquote

class BitTorrentSeeder:
    def __init__(self, port=6881, max_connections=50):
        self.port = port
        self.max_connections = max_connections
        self.peers = {}
        self.torrents = {}
        self.running = False
        
    def add_torrent(self, torrent_file, data_file):
        """添加要分享的 torrent"""
        torrent_data = self._parse_torrent(torrent_file)
        info_hash = self._calculate_info_hash(torrent_data[b'info'])
        
        self.torrents[info_hash] = {
            'torrent_data': torrent_data,
            'data_file': data_file,
            'pieces': self._load_pieces(data_file, torrent_data),
            'peers': set()
        }
        
        return info_hash
    
    def start_seeding(self):
        """開始做種服務"""
        self.running = True
        
        # 啟動服務器監聽
        server_socket = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        server_socket.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
        server_socket.bind(('0.0.0.0', self.port))
        server_socket.listen(self.max_connections)
        
        print(f"BitTorrent Seeder 開始監聽端口 {self.port}")
        
        while self.running:
            try:
                client_socket, address = server_socket.accept()
                
                # 為每個連接創建處理線程
                client_thread = threading.Thread(
                    target=self._handle_peer_connection,
                    args=(client_socket, address)
                )
                client_thread.daemon = True
                client_thread.start()
                
            except Exception as e:
                print(f"接受連接時出錯: {e}")
        
        server_socket.close()
    
    def _handle_peer_connection(self, client_socket, address):
        """處理對等點連接"""
        try:
            # BitTorrent 握手協議
            handshake = self._perform_handshake(client_socket)
            if not handshake:
                return
            
            info_hash = handshake['info_hash']
            peer_id = handshake['peer_id']
            
            if info_hash not in self.torrents:
                print(f"未知的 info_hash: {info_hash.hex()}")
                return
            
            # 處理消息循環
            self._message_loop(client_socket, info_hash, peer_id)
            
        except Exception as e:
            print(f"處理對等點連接時出錯: {e}")
        finally:
            client_socket.close()
    
    def _perform_handshake(self, client_socket):
        """執行 BitTorrent 握手"""
        # 接收握手消息
        handshake_data = client_socket.recv(68)
        if len(handshake_data) != 68:
            return None
        
        # 解析握手消息
        protocol_length = handshake_data[0]
        protocol = handshake_data[1:20]
        reserved = handshake_data[20:28]
        info_hash = handshake_data[28:48]
        peer_id = handshake_data[48:68]
        
        if protocol != b'BitTorrent protocol':
            return None
        
        # 發送握手回應
        our_peer_id = b'HIVEMIND_SEEDER_001'
        response = struct.pack('B', 19) + b'BitTorrent protocol' + b'\x00' * 8 + info_hash + our_peer_id
        client_socket.send(response)
        
        return {
            'info_hash': info_hash,
            'peer_id': peer_id
        }
    
    def _message_loop(self, client_socket, info_hash, peer_id):
        """處理 BitTorrent 消息循環"""
        torrent_info = self.torrents[info_hash]
        
        # 發送 bitfield 消息（表示我們有哪些塊）
        bitfield = self._create_bitfield(torrent_info)
        self._send_message(client_socket, 5, bitfield)  # 5 = bitfield
        
        while True:
            try:
                # 接收消息
                message = self._receive_message(client_socket)
                if not message:
                    break
                
                message_type = message['type']
                
                if message_type == 6:  # request
                    self._handle_request(client_socket, message['payload'], torrent_info)
                elif message_type == 2:  # interested
                    # 發送 unchoke
                    self._send_message(client_socket, 1, b'')  # 1 = unchoke
                elif message_type == 0:  # choke
                    pass
                elif message_type == 3:  # not interested
                    pass
                
            except socket.timeout:
                continue
            except Exception as e:
                print(f"消息處理錯誤: {e}")
                break
```

### 3. Tracker Server (`tracker.py`)
- **功能**: BitTorrent 追蹤服務器
- **狀態**: 基本實現
- **協議**: HTTP Tracker Protocol

**實現範例**:
```python
from http.server import HTTPServer, BaseHTTPRequestHandler
from urllib.parse import urlparse, parse_qs
import hashlib
import time
import json

class TrackerHandler(BaseHTTPRequestHandler):
    torrents = {}  # info_hash -> torrent_info
    peers = {}     # info_hash -> {peer_id: peer_info}
    
    def do_GET(self):
        """處理 tracker 請求"""
        parsed_url = urlparse(self.path)
        
        if parsed_url.path == '/announce':
            self._handle_announce(parsed_url.query)
        elif parsed_url.path == '/scrape':
            self._handle_scrape(parsed_url.query)
        elif parsed_url.path == '/stats':
            self._handle_stats()
        else:
            self._send_error(404, "Not Found")
    
    def _handle_announce(self, query_string):
        """處理 announce 請求"""
        params = parse_qs(query_string)
        
        # 驗證必需參數
        required_params = ['info_hash', 'peer_id', 'port', 'uploaded', 'downloaded', 'left']
        for param in required_params:
            if param not in params:
                self._send_error(400, f"Missing parameter: {param}")
                return
        
        info_hash = params['info_hash'][0]
        peer_id = params['peer_id'][0]
        ip = self.client_address[0]
        port = int(params['port'][0])
        uploaded = int(params['uploaded'][0])
        downloaded = int(params['downloaded'][0])
        left = int(params['left'][0])
        
        event = params.get('event', [''])[0]
        
        # 更新對等點資訊
        if info_hash not in self.peers:
            self.peers[info_hash] = {}
        
        if event == 'stopped':
            # 移除對等點
            if peer_id in self.peers[info_hash]:
                del self.peers[info_hash][peer_id]
        else:
            # 添加或更新對等點
            self.peers[info_hash][peer_id] = {
                'ip': ip,
                'port': port,
                'uploaded': uploaded,
                'downloaded': downloaded,
                'left': left,
                'last_seen': time.time()
            }
        
        # 生成回應
        response = self._generate_announce_response(info_hash, peer_id)
        
        self.send_response(200)
        self.send_header('Content-Type', 'text/plain')
        self.end_headers()
        self.wfile.write(response)
    
    def _generate_announce_response(self, info_hash, requesting_peer_id):
        """生成 announce 回應"""
        peers_list = []
        
        if info_hash in self.peers:
            for peer_id, peer_info in self.peers[info_hash].items():
                if peer_id != requesting_peer_id:
                    # 清理過期的對等點
                    if time.time() - peer_info['last_seen'] < 1800:  # 30分鐘
                        peers_list.append({
                            'peer id': peer_id,
                            'ip': peer_info['ip'],
                            'port': peer_info['port']
                        })
        
        response_dict = {
            'interval': 1800,  # 30分鐘
            'peers': peers_list,
            'complete': len([p for p in self.peers.get(info_hash, {}).values() if p['left'] == 0]),
            'incomplete': len([p for p in self.peers.get(info_hash, {}).values() if p['left'] > 0])
        }
        
        return bencodepy.encode(response_dict)
```

### 4. 測試組件
- **測試執行檔**: `test.exe`
- **測試 Torrent**: `test.torrent`
- **狀態**: 測試環境已建立

## 🗂️ 文件結構

```
bt/
├── create_torrent.py          # Torrent 創建器
├── seeder.py                  # 種子服務器
├── tracker.py                 # 追蹤服務器
├── test.exe                   # 測試執行檔
└── test.torrent              # 測試 torrent 文件
```

## 🌐 P2P 網路整合

### DHT (分散式哈希表) 支援
```python
import socket
import struct
import random
from hashlib import sha1

class DHTNode:
    def __init__(self, node_id=None, port=6881):
        self.node_id = node_id or self._generate_node_id()
        self.port = port
        self.routing_table = {}
        self.storage = {}
        self.socket = None
        
    def _generate_node_id(self):
        """生成隨機節點 ID"""
        return sha1(str(random.random()).encode()).digest()
    
    def start(self):
        """啟動 DHT 節點"""
        self.socket = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
        self.socket.bind(('0.0.0.0', self.port))
        
        print(f"DHT 節點啟動，監聽端口 {self.port}")
        
        while True:
            try:
                data, address = self.socket.recvfrom(1024)
                self._handle_message(data, address)
            except Exception as e:
                print(f"DHT 消息處理錯誤: {e}")
    
    def announce_peer(self, info_hash, port):
        """在 DHT 網路中宣告對等點"""
        closest_nodes = self._find_closest_nodes(info_hash)
        
        for node_address in closest_nodes:
            self._send_announce_peer(node_address, info_hash, port)
    
    def find_peers(self, info_hash):
        """在 DHT 網路中尋找對等點"""
        closest_nodes = self._find_closest_nodes(info_hash)
        peers = []
        
        for node_address in closest_nodes:
            node_peers = self._query_get_peers(node_address, info_hash)
            peers.extend(node_peers)
        
        return peers
    
    def _find_closest_nodes(self, target_id):
        """尋找最接近目標 ID 的節點"""
        # 簡化實現：返回路由表中的所有節點
        return list(self.routing_table.keys())
    
    def _send_announce_peer(self, node_address, info_hash, port):
        """向節點發送 announce_peer 消息"""
        message = {
            't': b'aa',  # transaction id
            'y': b'q',   # query
            'q': b'announce_peer',
            'a': {
                'id': self.node_id,
                'info_hash': info_hash,
                'port': port,
                'token': b'aoeusnth'  # 簡化的 token
            }
        }
        
        encoded_message = bencodepy.encode(message)
        self.socket.sendto(encoded_message, node_address)
```

### 對等點發現機制
```python
class PeerDiscovery:
    def __init__(self, dht_node, tracker_urls):
        self.dht_node = dht_node
        self.tracker_urls = tracker_urls
        self.discovered_peers = {}
        
    def discover_peers(self, info_hash):
        """發現指定 torrent 的對等點"""
        all_peers = []
        
        # 1. 從 tracker 獲取對等點
        tracker_peers = self._get_peers_from_trackers(info_hash)
        all_peers.extend(tracker_peers)
        
        # 2. 從 DHT 獲取對等點
        dht_peers = self.dht_node.find_peers(info_hash)
        all_peers.extend(dht_peers)
        
        # 3. 從本地快取獲取對等點
        cached_peers = self.discovered_peers.get(info_hash, [])
        all_peers.extend(cached_peers)
        
        # 去重並過濾
        unique_peers = self._deduplicate_peers(all_peers)
        active_peers = self._filter_active_peers(unique_peers)
        
        # 更新快取
        self.discovered_peers[info_hash] = active_peers
        
        return active_peers
    
    def _get_peers_from_trackers(self, info_hash):
        """從 tracker 服務器獲取對等點"""
        peers = []
        
        for tracker_url in self.tracker_urls:
            try:
                tracker_peers = self._query_tracker(tracker_url, info_hash)
                peers.extend(tracker_peers)
            except Exception as e:
                print(f"查詢 tracker {tracker_url} 失敗: {e}")
        
        return peers
    
    def _filter_active_peers(self, peers):
        """過濾活躍的對等點"""
        active_peers = []
        
        for peer in peers:
            if self._is_peer_active(peer):
                active_peers.append(peer)
        
        return active_peers
    
    def _is_peer_active(self, peer):
        """檢查對等點是否活躍"""
        try:
            # 嘗試連接對等點
            test_socket = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
            test_socket.settimeout(5)
            test_socket.connect((peer['ip'], peer['port']))
            test_socket.close()
            return True
        except:
            return False
```

## 📊 下載管理系統

### 分片下載管理器
```python
import threading
import queue
import time

class PieceDownloadManager:
    def __init__(self, torrent_info, output_file):
        self.torrent_info = torrent_info
        self.output_file = output_file
        self.piece_length = torrent_info['piece length']
        self.total_pieces = len(torrent_info['pieces']) // 20  # SHA1 = 20 bytes
        
        # 下載狀態
        self.downloaded_pieces = set()
        self.downloading_pieces = set()
        self.piece_queue = queue.Queue()
        self.download_threads = []
        
        # 統計資訊
        self.start_time = time.time()
        self.downloaded_bytes = 0
        self.total_bytes = torrent_info['length']
        
        # 初始化塊隊列
        for piece_index in range(self.total_pieces):
            self.piece_queue.put(piece_index)
    
    def start_download(self, peers, max_connections=5):
        """開始下載"""
        print(f"開始下載，共 {self.total_pieces} 個塊")
        
        # 創建下載線程
        for i in range(min(max_connections, len(peers))):
            if i < len(peers):
                thread = threading.Thread(
                    target=self._download_worker,
                    args=(peers[i],)
                )
                thread.daemon = True
                thread.start()
                self.download_threads.append(thread)
        
        # 等待下載完成
        while len(self.downloaded_pieces) < self.total_pieces:
            time.sleep(1)
            self._print_progress()
        
        print("下載完成！")
        self._save_file()
    
    def _download_worker(self, peer):
        """下載工作線程"""
        try:
            # 連接到對等點
            peer_connection = self._connect_to_peer(peer)
            if not peer_connection:
                return
            
            while True:
                try:
                    # 獲取要下載的塊
                    piece_index = self.piece_queue.get(timeout=5)
                    
                    if piece_index in self.downloaded_pieces:
                        continue
                    
                    self.downloading_pieces.add(piece_index)
                    
                    # 下載塊
                    piece_data = self._download_piece(peer_connection, piece_index)
                    
                    if piece_data and self._verify_piece(piece_index, piece_data):
                        self.downloaded_pieces.add(piece_index)
                        self.downloaded_bytes += len(piece_data)
                        self._save_piece(piece_index, piece_data)
                    else:
                        # 下載失敗，重新加入隊列
                        self.piece_queue.put(piece_index)
                    
                    self.downloading_pieces.discard(piece_index)
                    
                except queue.Empty:
                    break
                except Exception as e:
                    print(f"下載塊時出錯: {e}")
        
        except Exception as e:
            print(f"下載工作線程出錯: {e}")
    
    def _verify_piece(self, piece_index, piece_data):
        """驗證塊的完整性"""
        expected_hash = self.torrent_info['pieces'][piece_index * 20:(piece_index + 1) * 20]
        actual_hash = hashlib.sha1(piece_data).digest()
        return expected_hash == actual_hash
    
    def _print_progress(self):
        """打印下載進度"""
        progress = len(self.downloaded_pieces) / self.total_pieces * 100
        speed = self.downloaded_bytes / (time.time() - self.start_time + 1) / 1024  # KB/s
        
        print(f"下載進度: {progress:.1f}% ({len(self.downloaded_pieces)}/{self.total_pieces}) "
              f"速度: {speed:.1f} KB/s")
```

### 上傳管理器
```python
class PieceUploadManager:
    def __init__(self, torrent_info, file_path):
        self.torrent_info = torrent_info
        self.file_path = file_path
        self.upload_connections = {}
        self.upload_stats = {
            'uploaded_bytes': 0,
            'upload_requests': 0,
            'active_connections': 0
        }
    
    def handle_piece_request(self, peer_connection, piece_index, offset, length):
        """處理塊請求"""
        try:
            # 讀取請求的數據
            piece_data = self._read_piece_data(piece_index, offset, length)
            
            if piece_data:
                # 發送數據給對等點
                self._send_piece_data(peer_connection, piece_index, offset, piece_data)
                
                # 更新統計
                self.upload_stats['uploaded_bytes'] += len(piece_data)
                self.upload_stats['upload_requests'] += 1
                
                return True
            
        except Exception as e:
            print(f"處理塊請求時出錯: {e}")
        
        return False
    
    def _read_piece_data(self, piece_index, offset, length):
        """讀取指定塊的數據"""
        piece_length = self.torrent_info['piece length']
        file_offset = piece_index * piece_length + offset
        
        try:
            with open(self.file_path, 'rb') as f:
                f.seek(file_offset)
                return f.read(length)
        except Exception as e:
            print(f"讀取文件數據時出錯: {e}")
            return None
    
    def get_upload_statistics(self):
        """獲取上傳統計資訊"""
        return self.upload_stats.copy()
```

## 🔒 安全性和完整性

### 文件完整性驗證
```python
class IntegrityVerifier:
    def __init__(self, torrent_info):
        self.torrent_info = torrent_info
    
    def verify_complete_file(self, file_path):
        """驗證完整文件的完整性"""
        piece_length = self.torrent_info['piece length']
        total_length = self.torrent_info['length']
        expected_pieces = self.torrent_info['pieces']
        
        try:
            with open(file_path, 'rb') as f:
                piece_index = 0
                
                while True:
                    # 計算當前塊的大小
                    remaining_bytes = total_length - (piece_index * piece_length)
                    current_piece_length = min(piece_length, remaining_bytes)
                    
                    if current_piece_length <= 0:
                        break
                    
                    # 讀取塊數據
                    piece_data = f.read(current_piece_length)
                    if not piece_data:
                        break
                    
                    # 驗證哈希
                    expected_hash = expected_pieces[piece_index * 20:(piece_index + 1) * 20]
                    actual_hash = hashlib.sha1(piece_data).digest()
                    
                    if expected_hash != actual_hash:
                        return False, f"塊 {piece_index} 哈希不匹配"
                    
                    piece_index += 1
                
                return True, "文件完整性驗證通過"
                
        except Exception as e:
            return False, f"驗證過程中出錯: {e}"
    
    def verify_piece(self, piece_index, piece_data):
        """驗證單個塊的完整性"""
        expected_hash = self.torrent_info['pieces'][piece_index * 20:(piece_index + 1) * 20]
        actual_hash = hashlib.sha1(piece_data).digest()
        return expected_hash == actual_hash
```

### 對等點信任系統
```python
class PeerTrustManager:
    def __init__(self):
        self.peer_reputation = {}
        self.trust_factors = {
            'successful_downloads': 1.0,
            'failed_downloads': -2.0,
            'upload_ratio': 0.5,
            'connection_stability': 0.3
        }
    
    def update_peer_reputation(self, peer_id, event_type, value=1):
        """更新對等點信譽"""
        if peer_id not in self.peer_reputation:
            self.peer_reputation[peer_id] = {
                'successful_downloads': 0,
                'failed_downloads': 0,
                'upload_ratio': 1.0,
                'connection_stability': 1.0,
                'total_score': 0.0
            }
        
        reputation = self.peer_reputation[peer_id]
        
        if event_type in reputation:
            if event_type in ['successful_downloads', 'failed_downloads']:
                reputation[event_type] += value
            else:
                reputation[event_type] = value
        
        # 重新計算總分
        reputation['total_score'] = self._calculate_trust_score(reputation)
    
    def _calculate_trust_score(self, reputation):
        """計算信任分數"""
        score = 0.0
        
        for factor, weight in self.trust_factors.items():
            if factor in reputation:
                score += reputation[factor] * weight
        
        return max(0.0, min(10.0, score))  # 限制在 0-10 範圍
    
    def get_trusted_peers(self, peers, min_trust_score=5.0):
        """獲取可信任的對等點"""
        trusted_peers = []
        
        for peer in peers:
            peer_id = peer.get('peer_id') or f"{peer['ip']}:{peer['port']}"
            trust_score = self.peer_reputation.get(peer_id, {}).get('total_score', 5.0)
            
            if trust_score >= min_trust_score:
                trusted_peers.append(peer)
        
        return trusted_peers
```

## 📊 監控和指標

### BT 模組指標收集
```python
class BTMetricsCollector:
    def __init__(self):
        self.metrics = {
            'download_stats': {},
            'upload_stats': {},
            'peer_stats': {},
            'tracker_stats': {}
        }
    
    def collect_download_metrics(self, info_hash, metrics):
        """收集下載指標"""
        self.metrics['download_stats'][info_hash] = {
            'downloaded_bytes': metrics['downloaded_bytes'],
            'total_bytes': metrics['total_bytes'],
            'download_speed': metrics['download_speed'],
            'active_peers': metrics['active_peers'],
            'completion_percentage': metrics['completion_percentage'],
            'eta_seconds': metrics['eta_seconds'],
            'timestamp': time.time()
        }
    
    def collect_upload_metrics(self, info_hash, metrics):
        """收集上傳指標"""
        self.metrics['upload_stats'][info_hash] = {
            'uploaded_bytes': metrics['uploaded_bytes'],
            'upload_speed': metrics['upload_speed'],
            'active_connections': metrics['active_connections'],
            'upload_requests': metrics['upload_requests'],
            'timestamp': time.time()
        }
    
    def get_network_health(self):
        """獲取網路健康狀態"""
        total_downloads = len(self.metrics['download_stats'])
        total_uploads = len(self.metrics['upload_stats'])
        
        avg_download_speed = 0
        avg_upload_speed = 0
        
        if total_downloads > 0:
            total_download_speed = sum(
                stats['download_speed'] 
                for stats in self.metrics['download_stats'].values()
            )
            avg_download_speed = total_download_speed / total_downloads
        
        if total_uploads > 0:
            total_upload_speed = sum(
                stats['upload_speed'] 
                for stats in self.metrics['upload_stats'].values()
            )
            avg_upload_speed = total_upload_speed / total_uploads
        
        return {
            'active_torrents': total_downloads + total_uploads,
            'avg_download_speed_kbps': avg_download_speed,
            'avg_upload_speed_kbps': avg_upload_speed,
            'health_score': self._calculate_health_score()
        }
    
    def _calculate_health_score(self):
        """計算網路健康分數"""
        # 簡化的健康分數計算
        score = 100
        
        # 基於活躍的 torrent 數量
        active_torrents = len(self.metrics['download_stats']) + len(self.metrics['upload_stats'])
        if active_torrents == 0:
            score -= 50
        
        # 基於平均速度
        network_health = self.get_network_health()
        if network_health['avg_download_speed_kbps'] < 10:  # 10 KB/s
            score -= 20
        
        return max(0, score)
```

## 🔧 使用範例

### 創建和分享文件
```python
# 創建 torrent
creator = TorrentCreator("http://tracker.hivemind.local:8080/announce")
torrent_path = creator.create_torrent("large_dataset.zip")

# 啟動 seeder
seeder = BitTorrentSeeder(port=6881)
info_hash = seeder.add_torrent(torrent_path, "large_dataset.zip")
seeder.start_seeding()

print(f"開始分享文件，info_hash: {info_hash.hex()}")
```

### 下載文件
```python
# 發現對等點
discovery = PeerDiscovery(dht_node, ["http://tracker.hivemind.local:8080/announce"])
peers = discovery.discover_peers(info_hash)

# 開始下載
downloader = PieceDownloadManager(torrent_info, "downloaded_file.zip")
downloader.start_download(peers, max_connections=5)

# 驗證下載的文件
verifier = IntegrityVerifier(torrent_info)
is_valid, message = verifier.verify_complete_file("downloaded_file.zip")
print(f"文件驗證結果: {message}")
```

## 🔧 常見問題排除

### 1. Tracker 連接失敗
**問題**: 無法連接到 tracker 服務器
**解決**:
```python
def test_tracker_connection(tracker_url):
    try:
        response = requests.get(f"{tracker_url}/stats", timeout=10)
        if response.status_code == 200:
            print("Tracker 連接正常")
            return True
        else:
            print(f"Tracker 返回錯誤: {response.status_code}")
            return False
    except Exception as e:
        print(f"Tracker 連接失敗: {e}")
        return False
```

### 2. 對等點連接問題
**問題**: 無法連接到其他對等點
**解決**:
```python
def diagnose_peer_connectivity():
    # 檢查本地防火牆
    print("檢查防火牆設置...")
    
    # 檢查 NAT 設置
    print("檢查 NAT 配置...")
    
    # 測試端口開放性
    print("測試端口開放性...")
    
    # 建議解決方案
    print("建議啟用 UPnP 或手動設置端口轉發")
```

### 3. 下載速度慢
**問題**: 下載速度過慢
**解決**:
```python
def optimize_download_performance():
    # 增加最大連接數
    max_connections = 10
    
    # 優化塊選擇策略
    # 優先下載稀有塊
    
    # 啟用超級播種模式
    super_seeding = True
    
    # 調整請求管道深度
    request_pipeline_depth = 5
```

---

**相關文檔**:
- [TaskWorker 模組](taskworker.md)
- [AI Module 模組](ai.md)
- [Node Pool 模組](node-pool.md)
- [API 文檔](../api.md)
- [部署指南](../deployment.md)
