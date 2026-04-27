from platform import node
import json
from pathlib import Path
import sys


def _get_base_dir() -> Path:
    if getattr(sys, "frozen", False):
        return Path(sys.executable).parent
    return Path(__file__).parent


def _locate_config_file(base_dir: Path) -> Path:
    candidates = [
        base_dir / "worker_credentials.json",
        base_dir / "hivemind_worker" / "worker_credentials.json",
        Path(__file__).parent / "worker_credentials.json",
        Path.home() / ".hivemind" / "worker_credentials.json",
    ]

    def _has_required_fields(path: Path) -> bool:
        try:
            with open(path, 'r', encoding='utf-8') as f:
                data = json.load(f)
            return (data.get("node_port") is not None) and bool(data.get("nodepool_address"))
        except Exception:
            return False

    # 先找「真的像完整設定」的檔案，避免被只含帳密的檔案覆蓋
    for candidate in candidates:
        if candidate.exists() and _has_required_fields(candidate):
            return candidate

    # 退而求其次：任何存在的檔案（供錯誤訊息使用或舊行為相容）
    for candidate in candidates:
        if candidate.exists():
            return candidate

    # 回傳第一個候選，供錯誤訊息使用
    return candidates[0]


# 讀取 worker_credentials.json 配置
def load_config(force_reload: bool = False):
    """從 worker_credentials.json 讀取配置。

    編譯後（frozen）會以 exe 所在目錄為基準尋找；同時相容舊版路徑
    hivemind_worker\worker_credentials.json。

    Args:
        force_reload: 強制重新讀取配置文件（用於動態更新）
    """
    base_dir = _get_base_dir()
    config_file = _locate_config_file(base_dir)
    
    if not config_file.exists():
        print(f"錯誤: 找不到配置文件 {config_file}")
        sys.exit(1)

    # 方便現場 debug：確認到底讀到哪個檔案
    if force_reload:
        print(f"[config] Reloading config from: {config_file}")
    else:
        print(f"[config] Loading config from: {config_file}")
    
    try:
        with open(config_file, 'r', encoding='utf-8') as f:
            config = json.load(f)
    except Exception as e:
        print(f"錯誤: 無法讀取 worker_credentials.json: {e}")
        sys.exit(1)
    
    # 必須從 JSON 讀取配置
    node_port = config.get("node_port")
    nodepool_address = config.get("nodepool_address")    
    if node_port is None:
        print("錯誤: worker_credentials.json 缺少 node_port 配置")
        sys.exit(1)
    
    if not nodepool_address:
        print("錯誤: worker_credentials.json 缺少 nodepool_address 配置")
        sys.exit(1)
    
    node_port = int(node_port)
    nodepool_address = nodepool_address.strip()
    node_id = f"worker-{node().split('.')[0]}-{node_port}"
    
    return node_port, nodepool_address, node_id

# 服務配置 - 啟動時讀取一次
NODE_PORT, NODEPOOL_ADDRESS, NODE_ID = load_config()

def reload_config():
    """動態重新載入配置（供運行時調用）"""
    global NODE_PORT, NODEPOOL_ADDRESS, NODE_ID
    NODE_PORT, NODEPOOL_ADDRESS, NODE_ID = load_config(force_reload=True)
    print(f"配置已重新載入: NodePool={NODEPOOL_ADDRESS}, Port={NODE_PORT}")
    return NODE_PORT, NODEPOOL_ADDRESS, NODE_ID

# 資源限制
MAX_TASK_FILE_SIZE_MB = 100
TASK_TIMEOUT_SECONDS = 3600
MAX_CONCURRENT_TASKS = 5
