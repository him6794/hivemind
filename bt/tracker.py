from flask import Flask, request, Response
import bencode
import logging
import time
from collections import defaultdict
import struct
import socket

app = Flask(__name__)
logging.basicConfig(level=logging.INFO, format='%(asctime)s - %(levelname)s - %(message)s')

PEER_TIMEOUT = 10 * 60  # 10 分鐘
peers = defaultdict(lambda: defaultdict(dict))

def get_compact_peer_list(infohash):
    """將 peer 列表轉換為緊湊格式 (compact format)。"""
    compact_peers = b''
    current_time = time.time()
    
    # 創建副本以安全地迭代和刪除
    peer_list = list(peers[infohash].items())
    for peer_id, data in peer_list:
        if current_time - data['timestamp'] > PEER_TIMEOUT:
            del peers[infohash][peer_id]
            continue
        try:
            ip_bytes = socket.inet_aton(data['ip'])
            port_bytes = struct.pack("!H", data['port'])
            compact_peers += ip_bytes + port_bytes
        except socket.error:
            logging.warning(f"Invalid IP address for peer {peer_id}: {data['ip']}")
            continue
            
    return compact_peers

def tracker_error_response(message):
    """生成一個 bencoded 格式的錯誤回應。"""
    return Response(bencode.bencode({b'failure reason': message.encode('utf-8')}), mimetype='text/plain')

@app.route('/announce', methods=['GET'])
def announce():
    """處理 announce 請求。"""
    # 獲取必要的參數
    info_hash_bytes = request.args.get('info_hash', type=lambda x: x.encode('latin1'))
    peer_id_bytes = request.args.get('peer_id', type=lambda x: x.encode('latin1'))
    port = request.args.get('port', type=int)

    # 驗證參數是否存在
    if not all([info_hash_bytes, peer_id_bytes, port]):
        logging.warning(f"Invalid request: Missing required parameters from {request.remote_addr}")
        return tracker_error_response("Missing required parameters (info_hash, peer_id, port)")

    # --- 關鍵修正：驗證 info_hash 長度 ---
    if len(info_hash_bytes) != 20:
        logging.warning(f"Invalid request: Malformed info_hash from {request.remote_addr}")
        return tracker_error_response("Malformed info_hash")

    info_hash = info_hash_bytes.hex()
    peer_id = peer_id_bytes.hex()
    ip = request.remote_addr
    
    event = request.args.get('event')
    if event == 'stopped':
        if peer_id in peers[info_hash]:
            del peers[info_hash][peer_id]
        logging.info(f"Peer {peer_id} ({ip}:{port}) stopped on torrent {info_hash}")
    else:
        # 更新或添加 peer
        peers[info_hash][peer_id] = {
            'ip': ip,
            'port': port,
            'timestamp': time.time()
        }
        if event == 'started':
            logging.info(f"Peer {peer_id} ({ip}:{port}) started on torrent {info_hash}")
        else:
            logging.info(f"Announce from {peer_id} ({ip}:{port}) on torrent {info_hash}")

    # 創建並回傳 peer 列表
    compact_peer_list = get_compact_peer_list(info_hash)
    
    response_data = {
        b'interval': 300,
        b'peers': compact_peer_list,
        b'complete': len([p for p in peers[info_hash].values()]), # 做種者數量
        b'incomplete': 0 # 下載者數量 (簡化模型)
    }
    
    bencoded_response = bencode.bencode(response_data)
    return Response(bencoded_response, mimetype='text/plain')

@app.route('/scrape')
def scrape():
    # 簡單實現，可以擴展
    return Response(bencode.bencode({b'files': {}}), mimetype='text/plain')

@app.route('/')
def index():
    return "Private BitTorrent tracker is running"

if __name__ == '__main__':
    print("Private BitTorrent tracker is running...")
    print("Please point your torrent client to http://<your IP>:5000/announce")
    logging.info("Starting tracker...")
    app.run(host='0.0.0.0', port=5000, debug=False)