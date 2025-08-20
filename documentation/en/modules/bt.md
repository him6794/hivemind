# BT Module Documentation

## 📋 Overview

The BT (BitTorrent) Module is the peer-to-peer file transfer component of the HiveMind distributed computing platform, responsible for efficient distribution and sharing of large datasets and model files. Based on the BitTorrent protocol for decentralized file transfer.

## 🏗️ System Architecture

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

## 🔧 Core Components

### 1. Torrent Creator (`create_torrent.py`)
- **Function**: Create and manage torrent files
- **Status**: Basic implementation complete
- **Purpose**: Package large files as torrents for distribution

**Main Features**:
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
        """Create torrent file"""
        if not os.path.exists(file_path):
            raise FileNotFoundError(f"File not found: {file_path}")
        
        # Calculate file piece information
        pieces = self._calculate_pieces(file_path)
        file_size = os.path.getsize(file_path)
        
        # Build torrent dictionary
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
        
        # Encode and save
        encoded_data = bencodepy.encode(torrent_data)
        
        if output_path is None:
            output_path = file_path + '.torrent'
        
        with open(output_path, 'wb') as f:
            f.write(encoded_data)
        
        return output_path
    
    def _calculate_pieces(self, file_path):
        """Calculate file piece SHA1 hashes"""
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
- **Function**: BitTorrent seeder server
- **Status**: Core functionality implemented
- **Protocol**: BitTorrent Protocol

**Implementation Example**:
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
        """Add torrent to share"""
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
        """Start seeding service"""
        self.running = True
        
        # Start server listening
        server_socket = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        server_socket.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
        server_socket.bind(('0.0.0.0', self.port))
        server_socket.listen(self.max_connections)
        
        print(f"BitTorrent Seeder listening on port {self.port}")
        
        while self.running:
            try:
                client_socket, address = server_socket.accept()
                
                # Create handler thread for each connection
                client_thread = threading.Thread(
                    target=self._handle_peer_connection,
                    args=(client_socket, address)
                )
                client_thread.daemon = True
                client_thread.start()
                
            except Exception as e:
                print(f"Error accepting connection: {e}")
        
        server_socket.close()
    
    def _handle_peer_connection(self, client_socket, address):
        """Handle peer connection"""
        try:
            # BitTorrent handshake protocol
            handshake = self._perform_handshake(client_socket)
            if not handshake:
                return
            
            info_hash = handshake['info_hash']
            peer_id = handshake['peer_id']
            
            if info_hash not in self.torrents:
                print(f"Unknown info_hash: {info_hash.hex()}")
                return
            
            # Message loop
            self._message_loop(client_socket, info_hash, peer_id)
            
        except Exception as e:
            print(f"Error handling peer connection: {e}")
        finally:
            client_socket.close()
    
    def _perform_handshake(self, client_socket):
        """Perform BitTorrent handshake"""
        # Receive handshake message
        handshake_data = client_socket.recv(68)
        if len(handshake_data) != 68:
            return None
        
        # Parse handshake message
        protocol_length = handshake_data[0]
        protocol = handshake_data[1:20]
        reserved = handshake_data[20:28]
        info_hash = handshake_data[28:48]
        peer_id = handshake_data[48:68]
        
        if protocol != b'BitTorrent protocol':
            return None
        
        # Send handshake response
        our_peer_id = b'HIVEMIND_SEEDER_001'
        response = struct.pack('B', 19) + b'BitTorrent protocol' + b'\x00' * 8 + info_hash + our_peer_id
        client_socket.send(response)
        
        return {
            'info_hash': info_hash,
            'peer_id': peer_id
        }
    
    def _message_loop(self, client_socket, info_hash, peer_id):
        """Handle BitTorrent message loop"""
        torrent_info = self.torrents[info_hash]
        
        # Send bitfield message (indicates which pieces we have)
        bitfield = self._create_bitfield(torrent_info)
        self._send_message(client_socket, 5, bitfield)  # 5 = bitfield
        
        while True:
            try:
                # Receive message
                message = self._receive_message(client_socket)
                if not message:
                    break
                
                message_type = message['type']
                
                if message_type == 6:  # request
                    self._handle_request(client_socket, message['payload'], torrent_info)
                elif message_type == 2:  # interested
                    # Send unchoke
                    self._send_message(client_socket, 1, b'')  # 1 = unchoke
                elif message_type == 0:  # choke
                    pass
                elif message_type == 3:  # not interested
                    pass
                
            except socket.timeout:
                continue
            except Exception as e:
                print(f"Message handling error: {e}")
                break
```

### 3. Tracker Server (`tracker.py`)
- **Function**: BitTorrent tracker server
- **Status**: Basic implementation
- **Protocol**: HTTP Tracker Protocol

**Implementation Example**:
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
        """Handle tracker requests"""
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
        """Handle announce request"""
        params = parse_qs(query_string)
        
        # Validate required parameters
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
        
        # Update peer information
        if info_hash not in self.peers:
            self.peers[info_hash] = {}
        
        if event == 'stopped':
            # Remove peer
            if peer_id in self.peers[info_hash]:
                del self.peers[info_hash][peer_id]
        else:
            # Add or update peer
            self.peers[info_hash][peer_id] = {
                'ip': ip,
                'port': port,
                'uploaded': uploaded,
                'downloaded': downloaded,
                'left': left,
                'last_seen': time.time()
            }
        
        # Generate response
        response = self._generate_announce_response(info_hash, peer_id)
        
        self.send_response(200)
        self.send_header('Content-Type', 'text/plain')
        self.end_headers()
        self.wfile.write(response)
    
    def _generate_announce_response(self, info_hash, requesting_peer_id):
        """Generate announce response"""
        peers_list = []
        
        if info_hash in self.peers:
            for peer_id, peer_info in self.peers[info_hash].items():
                if peer_id != requesting_peer_id:
                    # Clean up expired peers
                    if time.time() - peer_info['last_seen'] < 1800:  # 30 minutes
                        peers_list.append({
                            'peer id': peer_id,
                            'ip': peer_info['ip'],
                            'port': peer_info['port']
                        })
        
        response_dict = {
            'interval': 1800,  # 30 minutes
            'peers': peers_list,
            'complete': len([p for p in self.peers.get(info_hash, {}).values() if p['left'] == 0]),
            'incomplete': len([p for p in self.peers.get(info_hash, {}).values() if p['left'] > 0])
        }
        
        return bencodepy.encode(response_dict)
```

### 4. Test Components
- **Test Executable**: `test.exe`
- **Test Torrent**: `test.torrent`
- **Status**: Test environment established

## 🗂️ File Structure

```
bt/
├── create_torrent.py          # Torrent creator
├── seeder.py                  # Seeder server
├── tracker.py                 # Tracker server
├── test.exe                   # Test executable
└── test.torrent              # Test torrent file
```

## 🌐 P2P Network Integration

### DHT (Distributed Hash Table) Support
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
        """Generate random node ID"""
        return sha1(str(random.random()).encode()).digest()
    
    def start(self):
        """Start DHT node"""
        self.socket = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
        self.socket.bind(('0.0.0.0', self.port))
        
        print(f"DHT node started, listening on port {self.port}")
        
        while True:
            try:
                data, address = self.socket.recvfrom(1024)
                self._handle_message(data, address)
            except Exception as e:
                print(f"DHT message handling error: {e}")
    
    def announce_peer(self, info_hash, port):
        """Announce peer in DHT network"""
        closest_nodes = self._find_closest_nodes(info_hash)
        
        for node_address in closest_nodes:
            self._send_announce_peer(node_address, info_hash, port)
    
    def find_peers(self, info_hash):
        """Find peers in DHT network"""
        closest_nodes = self._find_closest_nodes(info_hash)
        peers = []
        
        for node_address in closest_nodes:
            node_peers = self._query_get_peers(node_address, info_hash)
            peers.extend(node_peers)
        
        return peers
    
    def _find_closest_nodes(self, target_id):
        """Find nodes closest to target ID"""
        # Simplified implementation: return all nodes in routing table
        return list(self.routing_table.keys())
    
    def _send_announce_peer(self, node_address, info_hash, port):
        """Send announce_peer message to node"""
        message = {
            't': b'aa',  # transaction id
            'y': b'q',   # query
            'q': b'announce_peer',
            'a': {
                'id': self.node_id,
                'info_hash': info_hash,
                'port': port,
                'token': b'aoeusnth'  # Simplified token
            }
        }
        
        encoded_message = bencodepy.encode(message)
        self.socket.sendto(encoded_message, node_address)
```

### Peer Discovery Mechanism
```python
class PeerDiscovery:
    def __init__(self, dht_node, tracker_urls):
        self.dht_node = dht_node
        self.tracker_urls = tracker_urls
        self.discovered_peers = {}
        
    def discover_peers(self, info_hash):
        """Discover peers for specified torrent"""
        all_peers = []
        
        # 1. Get peers from trackers
        tracker_peers = self._get_peers_from_trackers(info_hash)
        all_peers.extend(tracker_peers)
        
        # 2. Get peers from DHT
        dht_peers = self.dht_node.find_peers(info_hash)
        all_peers.extend(dht_peers)
        
        # 3. Get peers from local cache
        cached_peers = self.discovered_peers.get(info_hash, [])
        all_peers.extend(cached_peers)
        
        # Deduplicate and filter
        unique_peers = self._deduplicate_peers(all_peers)
        active_peers = self._filter_active_peers(unique_peers)
        
        # Update cache
        self.discovered_peers[info_hash] = active_peers
        
        return active_peers
    
    def _get_peers_from_trackers(self, info_hash):
        """Get peers from tracker servers"""
        peers = []
        
        for tracker_url in self.tracker_urls:
            try:
                tracker_peers = self._query_tracker(tracker_url, info_hash)
                peers.extend(tracker_peers)
            except Exception as e:
                print(f"Query tracker {tracker_url} failed: {e}")
        
        return peers
    
    def _filter_active_peers(self, peers):
        """Filter active peers"""
        active_peers = []
        
        for peer in peers:
            if self._is_peer_active(peer):
                active_peers.append(peer)
        
        return active_peers
    
    def _is_peer_active(self, peer):
        """Check if peer is active"""
        try:
            # Try to connect to peer
            test_socket = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
            test_socket.settimeout(5)
            test_socket.connect((peer['ip'], peer['port']))
            test_socket.close()
            return True
        except:
            return False
```

## 📊 Download Management System

### Piece Download Manager
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
        
        # Download status
        self.downloaded_pieces = set()
        self.downloading_pieces = set()
        self.piece_queue = queue.Queue()
        self.download_threads = []
        
        # Statistics
        self.start_time = time.time()
        self.downloaded_bytes = 0
        self.total_bytes = torrent_info['length']
        
        # Initialize piece queue
        for piece_index in range(self.total_pieces):
            self.piece_queue.put(piece_index)
    
    def start_download(self, peers, max_connections=5):
        """Start download"""
        print(f"Starting download, {self.total_pieces} pieces total")
        
        # Create download threads
        for i in range(min(max_connections, len(peers))):
            if i < len(peers):
                thread = threading.Thread(
                    target=self._download_worker,
                    args=(peers[i],)
                )
                thread.daemon = True
                thread.start()
                self.download_threads.append(thread)
        
        # Wait for download completion
        while len(self.downloaded_pieces) < self.total_pieces:
            time.sleep(1)
            self._print_progress()
        
        print("Download completed!")
        self._save_file()
    
    def _download_worker(self, peer):
        """Download worker thread"""
        try:
            # Connect to peer
            peer_connection = self._connect_to_peer(peer)
            if not peer_connection:
                return
            
            while True:
                try:
                    # Get piece to download
                    piece_index = self.piece_queue.get(timeout=5)
                    
                    if piece_index in self.downloaded_pieces:
                        continue
                    
                    self.downloading_pieces.add(piece_index)
                    
                    # Download piece
                    piece_data = self._download_piece(peer_connection, piece_index)
                    
                    if piece_data and self._verify_piece(piece_index, piece_data):
                        self.downloaded_pieces.add(piece_index)
                        self.downloaded_bytes += len(piece_data)
                        self._save_piece(piece_index, piece_data)
                    else:
                        # Download failed, re-queue
                        self.piece_queue.put(piece_index)
                    
                    self.downloading_pieces.discard(piece_index)
                    
                except queue.Empty:
                    break
                except Exception as e:
                    print(f"Error downloading piece: {e}")
        
        except Exception as e:
            print(f"Download worker error: {e}")
    
    def _verify_piece(self, piece_index, piece_data):
        """Verify piece integrity"""
        expected_hash = self.torrent_info['pieces'][piece_index * 20:(piece_index + 1) * 20]
        actual_hash = hashlib.sha1(piece_data).digest()
        return expected_hash == actual_hash
    
    def _print_progress(self):
        """Print download progress"""
        progress = len(self.downloaded_pieces) / self.total_pieces * 100
        speed = self.downloaded_bytes / (time.time() - self.start_time + 1) / 1024  # KB/s
        
        print(f"Download progress: {progress:.1f}% ({len(self.downloaded_pieces)}/{self.total_pieces}) "
              f"Speed: {speed:.1f} KB/s")
```

### Upload Manager
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
        """Handle piece request"""
        try:
            # Read requested data
            piece_data = self._read_piece_data(piece_index, offset, length)
            
            if piece_data:
                # Send data to peer
                self._send_piece_data(peer_connection, piece_index, offset, piece_data)
                
                # Update statistics
                self.upload_stats['uploaded_bytes'] += len(piece_data)
                self.upload_stats['upload_requests'] += 1
                
                return True
            
        except Exception as e:
            print(f"Error handling piece request: {e}")
        
        return False
    
    def _read_piece_data(self, piece_index, offset, length):
        """Read specified piece data"""
        piece_length = self.torrent_info['piece length']
        file_offset = piece_index * piece_length + offset
        
        try:
            with open(self.file_path, 'rb') as f:
                f.seek(file_offset)
                return f.read(length)
        except Exception as e:
            print(f"Error reading file data: {e}")
            return None
    
    def get_upload_statistics(self):
        """Get upload statistics"""
        return self.upload_stats.copy()
```

## 🔒 Security and Integrity

### File Integrity Verification
```python
class IntegrityVerifier:
    def __init__(self, torrent_info):
        self.torrent_info = torrent_info
    
    def verify_complete_file(self, file_path):
        """Verify complete file integrity"""
        piece_length = self.torrent_info['piece length']
        total_length = self.torrent_info['length']
        expected_pieces = self.torrent_info['pieces']
        
        try:
            with open(file_path, 'rb') as f:
                piece_index = 0
                
                while True:
                    # Calculate current piece size
                    remaining_bytes = total_length - (piece_index * piece_length)
                    current_piece_length = min(piece_length, remaining_bytes)
                    
                    if current_piece_length <= 0:
                        break
                    
                    # Read piece data
                    piece_data = f.read(current_piece_length)
                    if not piece_data:
                        break
                    
                    # Verify hash
                    expected_hash = expected_pieces[piece_index * 20:(piece_index + 1) * 20]
                    actual_hash = hashlib.sha1(piece_data).digest()
                    
                    if expected_hash != actual_hash:
                        return False, f"Piece {piece_index} hash mismatch"
                    
                    piece_index += 1
                
                return True, "File integrity verification passed"
                
        except Exception as e:
            return False, f"Error during verification: {e}"
    
    def verify_piece(self, piece_index, piece_data):
        """Verify single piece integrity"""
        expected_hash = self.torrent_info['pieces'][piece_index * 20:(piece_index + 1) * 20]
        actual_hash = hashlib.sha1(piece_data).digest()
        return expected_hash == actual_hash
```

### Peer Trust System
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
        """Update peer reputation"""
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
        
        # Recalculate total score
        reputation['total_score'] = self._calculate_trust_score(reputation)
    
    def _calculate_trust_score(self, reputation):
        """Calculate trust score"""
        score = 0.0
        
        for factor, weight in self.trust_factors.items():
            if factor in reputation:
                score += reputation[factor] * weight
        
        return max(0.0, min(10.0, score))  # Limit to 0-10 range
    
    def get_trusted_peers(self, peers, min_trust_score=5.0):
        """Get trusted peers"""
        trusted_peers = []
        
        for peer in peers:
            peer_id = peer.get('peer_id') or f"{peer['ip']}:{peer['port']}"
            trust_score = self.peer_reputation.get(peer_id, {}).get('total_score', 5.0)
            
            if trust_score >= min_trust_score:
                trusted_peers.append(peer)
        
        return trusted_peers
```

## 📊 Monitoring and Metrics

### BT Module Metrics Collection
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
        """Collect download metrics"""
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
        """Collect upload metrics"""
        self.metrics['upload_stats'][info_hash] = {
            'uploaded_bytes': metrics['uploaded_bytes'],
            'upload_speed': metrics['upload_speed'],
            'active_connections': metrics['active_connections'],
            'upload_requests': metrics['upload_requests'],
            'timestamp': time.time()
        }
    
    def get_network_health(self):
        """Get network health status"""
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
        """Calculate network health score"""
        # Simplified health score calculation
        score = 100
        
        # Based on active torrent count
        active_torrents = len(self.metrics['download_stats']) + len(self.metrics['upload_stats'])
        if active_torrents == 0:
            score -= 50
        
        # Based on average speed
        network_health = self.get_network_health()
        if network_health['avg_download_speed_kbps'] < 10:  # 10 KB/s
            score -= 20
        
        return max(0, score)
```

## 🔧 Usage Examples

### Create and Share Files
```python
# Create torrent
creator = TorrentCreator("http://tracker.hivemind.local:8080/announce")
torrent_path = creator.create_torrent("large_dataset.zip")

# Start seeder
seeder = BitTorrentSeeder(port=6881)
info_hash = seeder.add_torrent(torrent_path, "large_dataset.zip")
seeder.start_seeding()

print(f"Started sharing file, info_hash: {info_hash.hex()}")
```

### Download Files
```python
# Discover peers
discovery = PeerDiscovery(dht_node, ["http://tracker.hivemind.local:8080/announce"])
peers = discovery.discover_peers(info_hash)

# Start download
downloader = PieceDownloadManager(torrent_info, "downloaded_file.zip")
downloader.start_download(peers, max_connections=5)

# Verify downloaded file
verifier = IntegrityVerifier(torrent_info)
is_valid, message = verifier.verify_complete_file("downloaded_file.zip")
print(f"File verification result: {message}")
```

## 🔧 Common Troubleshooting

### 1. Tracker Connection Failure
**Problem**: Unable to connect to tracker server
**Solution**:
```python
def test_tracker_connection(tracker_url):
    try:
        response = requests.get(f"{tracker_url}/stats", timeout=10)
        if response.status_code == 200:
            print("Tracker connection OK")
            return True
        else:
            print(f"Tracker returned error: {response.status_code}")
            return False
    except Exception as e:
        print(f"Tracker connection failed: {e}")
        return False
```

### 2. Peer Connection Issues
**Problem**: Unable to connect to other peers
**Solution**:
```python
def diagnose_peer_connectivity():
    # Check local firewall
    print("Checking firewall settings...")
    
    # Check NAT settings
    print("Checking NAT configuration...")
    
    # Test port openness
    print("Testing port openness...")
    
    # Suggest solutions
    print("Recommend enabling UPnP or manual port forwarding")
```

### 3. Slow Download Speed
**Problem**: Download speed too slow
**Solution**:
```python
def optimize_download_performance():
    # Increase max connections
    max_connections = 10
    
    # Optimize piece selection strategy
    # Prioritize rare pieces
    
    # Enable super seeding mode
    super_seeding = True
    
    # Adjust request pipeline depth
    request_pipeline_depth = 5
```

---

**Related Documentation**:
- [TaskWorker Module](taskworker.md)
- [AI Module](ai.md)
- [Node Pool Module](node-pool.md)
- [API Documentation](../api.md)
- [Deployment Guide](../deployment.md)
