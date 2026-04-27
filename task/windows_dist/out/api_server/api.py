from flask import Flask, request, jsonify, render_template
import sys
import threading
import time
import json
import os
from pathlib import Path
from datetime import datetime

app = Flask(__name__)

# 全域錯誤處理：確保 API 異常時回 JSON 而非 HTML
@app.errorhandler(Exception)
def handle_error(e):
    try:
        code = getattr(e, 'code', 500)
    except Exception:
        code = 500
    return jsonify({
        'error': str(e),
        'type': type(e).__name__,
    }), code

# Global variables
power_n = None
num_chunks = None
remaining_chunks = []
global_primes = []
lock = threading.Lock()
tasks_completed = 0
start_time = None
durations = []
task_running = False
task_initialized = False
total_numbers = 0
processed_numbers = 0
max_prime_seen = None

# 任務狀態持久化與租約
STATE_FILE = Path(__file__).resolve().parent / 'task_state.json'
RUN_RESULTS_FILE = Path(__file__).resolve().parent / 'run_results.ndjson'
QUEUE_FILE = Path(__file__).resolve().parent / 'job_queue.json'
LEASE_SECONDS = 20 * 60
in_flight = {}
completed_task_ids = set()
# 每個 task_id 的已回報進度游標（下一個尚未處理的起點；以 window_end（exclusive）推進）
task_cursors = {}

# 任務隊列（用於重複測試 / 多次 run）
job_queue: list[dict] = []
current_run: dict | None = None
_run_seq = 0

# 控制同時參與運算的 worker 台數（0/None = 不限制）
max_active_workers: int | None = None
active_workers: set[str] = set()


def _save_queue() -> None:
    """Persist job_queue separately so frequent _save_state() doesn't rewrite huge lists."""
    try:
        tmp = str(QUEUE_FILE) + '.tmp'
        payload = {
            'job_queue': job_queue,
        }
        with open(tmp, 'w', encoding='utf-8') as f:
            json.dump(payload, f, ensure_ascii=False)
        os.replace(tmp, QUEUE_FILE)
    except Exception:
        pass


def _load_queue() -> None:
    global job_queue
    try:
        if not QUEUE_FILE.exists():
            return
        with open(QUEUE_FILE, 'r', encoding='utf-8') as f:
            payload = json.load(f) or {}
        job_queue = payload.get('job_queue') or []
    except Exception:
        job_queue = []


def _num_chunks_int() -> int:
    try:
        return int(num_chunks or 0)
    except Exception:
        return 0

# 批次寫檔緩衝，降低大量小寫入造成的 I/O 開銷
_primes_lines_buffer = []
_PRIMES_FLUSH_LINES = 1000


def _flush_primes_lines(force: bool = False) -> None:
    global _primes_lines_buffer
    if not _primes_lines_buffer:
        return
    if not force and len(_primes_lines_buffer) < _PRIMES_FLUSH_LINES:
        return
    try:
        with open('primes_reports.ndjson', 'a', encoding='utf-8') as f:
            f.writelines(_primes_lines_buffer)
        _primes_lines_buffer = []
    except Exception:
        # 寫檔失敗就清掉避免卡住
        _primes_lines_buffer = []


def _save_state() -> None:
    try:
        state = {
            'power_n': power_n,
            'num_chunks': num_chunks,
            'remaining_chunks': remaining_chunks,
            'in_flight': in_flight,
            'completed_task_ids': sorted(completed_task_ids),
            'task_cursors': task_cursors,
            'tasks_completed': tasks_completed,
            'start_time': start_time,
            'durations': durations,
            'task_running': task_running,
            'task_initialized': task_initialized,
            'total_numbers': total_numbers,
            'processed_numbers': processed_numbers,
            'buffered_primes_count': buffered_primes_count,
            'max_prime_seen': max_prime_seen,
            # queue / run（job_queue 另存 QUEUE_FILE；這裡只存長度避免頻繁寫爆）
            'queue_length': len(job_queue),
            'current_run': current_run,
            'run_seq': _run_seq,
            'max_active_workers': max_active_workers,
            'active_workers': sorted(active_workers),
        }
        tmp = str(STATE_FILE) + '.tmp'
        with open(tmp, 'w', encoding='utf-8') as f:
            json.dump(state, f, ensure_ascii=False)
        os.replace(tmp, STATE_FILE)
    except Exception:
        pass


def _load_state() -> bool:
    global power_n, num_chunks, remaining_chunks, in_flight, completed_task_ids
    global tasks_completed, start_time, durations, task_running, task_initialized
    global total_numbers, processed_numbers, buffered_primes_count
    global task_cursors
    global max_prime_seen
    global current_run, _run_seq, max_active_workers, active_workers

    try:
        if not STATE_FILE.exists():
            return False
        with open(STATE_FILE, 'r', encoding='utf-8') as f:
            state = json.load(f)

        power_n = state.get('power_n')
        num_chunks = state.get('num_chunks')
        remaining_chunks = state.get('remaining_chunks') or []
        in_flight = state.get('in_flight') or {}
        completed_task_ids = set(state.get('completed_task_ids') or [])
        task_cursors = state.get('task_cursors') or {}
        tasks_completed = int(state.get('tasks_completed') or 0)
        start_time = state.get('start_time')
        durations = state.get('durations') or []
        task_running = bool(state.get('task_running'))
        task_initialized = bool(state.get('task_initialized'))
        total_numbers = int(state.get('total_numbers') or 0)
        processed_numbers = int(state.get('processed_numbers') or 0)
        buffered_primes_count = int(state.get('buffered_primes_count') or 0)
        max_prime_seen = state.get('max_prime_seen', None)

        current_run = state.get('current_run', None)
        _run_seq = int(state.get('run_seq') or 0)
        maw = state.get('max_active_workers', None)
        try:
            max_active_workers = int(maw) if maw is not None else None
        except Exception:
            max_active_workers = None
        active_workers = set(state.get('active_workers') or [])

        # queue lives in separate file
        _load_queue()
        return True
    except Exception:
        return False


def _peek_state_power_n() -> int | None:
    try:
        if not STATE_FILE.exists():
            return None
        with open(STATE_FILE, 'r', encoding='utf-8') as f:
            state = json.load(f)
        v = state.get('power_n')
        return int(v) if v is not None else None
    except Exception:
        return None


def _requeue_expired_inflight(now_ts: float) -> None:
    # 將超時未完成的任務丟回 remaining
    try:
        expired = []
        for task_id, info in list(in_flight.items()):
            assigned_at = float(info.get('assigned_at') or 0)
            if assigned_at and (now_ts - assigned_at) > LEASE_SECONDS:
                expired.append(task_id)
        for task_id in expired:
            task = (in_flight.get(task_id) or {}).get('task')
            if task:
                remaining_chunks.append(task)
            in_flight.pop(task_id, None)
    except Exception:
        pass


def _expected_range_end(power: int) -> int:
    try:
        return 10 ** int(power)
    except Exception:
        return 0


def _max_task_end_from_state() -> int:
    ends = []
    try:
        for t in (remaining_chunks or []):
            try:
                ends.append(int(t.get('end') or 0))
            except Exception:
                pass
        for _, info in (in_flight or {}).items():
            task = (info or {}).get('task') or {}
            try:
                ends.append(int(task.get('end') or 0))
            except Exception:
                pass
    except Exception:
        pass
    return max(ends) if ends else 0


def _state_sane_for_power(power: int) -> tuple[bool, str]:
    """粗略檢查 state 是否與 power 對得上，避免錯誤續算。"""
    expected_end = _expected_range_end(power)
    max_end = _max_task_end_from_state()

    # 若還沒初始化或沒有 state 任務資訊，先視為 sane（交由其他欄位判斷）
    if not task_initialized:
        return True, 'not_initialized'

    # 若 num_chunks>0 但 tasks_completed<num_chunks，卻同時沒有 remaining/in_flight，表示 state 不一致
    try:
        chunks = int(_num_chunks_int())
        if chunks > 0 and int(tasks_completed or 0) < chunks and (len(remaining_chunks) == 0) and (len(in_flight) == 0):
            return False, 'no_tasks_but_incomplete'
    except Exception:
        pass

    # 若有 task end 記錄，應該至少到達 expected_end（chunks 切分下最大 end 會是 10^n）
    if max_end and expected_end and max_end < expected_end:
        return False, f'max_task_end_too_small:{max_end}<{expected_end}'

    return True, 'ok'


def _build_numeric_tasks(power: int, chunks: int, align: int = 1000) -> list[dict]:
    start = 1
    end = 10 ** int(power)
    total = end - start
    if chunks <= 0:
        chunks = 1
    # ceil(total/chunks)
    base = (total + chunks - 1) // chunks
    if align > 1:
        base = ((base + align - 1) // align) * align
    tasks = []
    cur = start
    i = 0
    while cur < end:
        nxt = min(cur + base, end)
        task_id = f"{power}:{i}:{cur}-{nxt}"
        tasks.append({'task_id': task_id, 'start': int(cur), 'end': int(nxt)})
        cur = nxt
        i += 1
    return tasks

# 緩衝機制：先在記憶體緩衝收到的質數，當緩衝超過閾值或所有任務完成時，將緩衝內容追加寫入磁碟
buffered_primes = []
buffered_primes_count = 0  # 只統計數量，不儲存所有質數
BUFFER_FLUSH_THRESHOLD = 10000  # 超過此數量就寫入磁碟（可調）
APPEND_FILE = 'primes_partial.ndjson'  # 每個報告以 ndjson 追加
SAVE_ACTUAL_PRIMES = False  # 是否儲存實際質數（大規模計算時應設為 False）

def _flush_buffer_to_disk(force=False):
    """將緩衝的 primes 追加寫入磁碟，並清空緩衝（除非 force=False 而未達閾值）"""
    global buffered_primes
    if not buffered_primes:
        return
    if not force and len(buffered_primes) < BUFFER_FLUSH_THRESHOLD:
        return

    try:
        # 以追加方式寫入，每行一個 JSON 陣列或整數（為了簡單採用一行一個整數）
        with open(APPEND_FILE, 'a', encoding='utf-8') as f:
            for p in buffered_primes:
                f.write(f"{p}\n")
        buffered_primes = []
    except Exception as e:
        print(f"Failed to flush buffer to disk: {e}")

def init_server_from_params(n):
    """Initialize server with power n (range 1 to 10^n)"""
    global power_n, num_chunks, remaining_chunks
    global start_time, tasks_completed, durations, buffered_primes, buffered_primes_count, task_running, task_initialized
    global total_numbers, processed_numbers
    global in_flight, completed_task_ids
    global task_cursors
    global max_prime_seen
    global active_workers
    
    power_n = int(n)
    # 先預設為 1 chunk；真正 chunks 由 /start_task 傳入時覆寫
    remaining_chunks = _build_numeric_tasks(power_n, chunks=1, align=1000)
    num_chunks = len(remaining_chunks)
    in_flight = {}
    completed_task_ids = set()
    task_cursors = {}
    active_workers = set()
    
    # 開始時間改為「真正開始派發/計算」才設定（避免 UI 一按就開始計時）
    start_time = None
    tasks_completed = 0
    durations = []
    buffered_primes = []
    buffered_primes_count = 0
    max_prime_seen = None
    total_numbers = 0
    processed_numbers = 0
    task_running = True
    task_initialized = True
    
    # remove previous partial file if exists to start fresh
    try:
        if os.path.exists(APPEND_FILE):
            os.remove(APPEND_FILE)
    except Exception:
        pass
    print(f"Server initialized for range 1 to 10^{power_n}, divided into {num_chunks} chunks.")

    try:
        total_numbers = int((10 ** power_n) - 1)
    except Exception:
        total_numbers = 0

    _save_state()


def _utc_iso(ts: float | None = None) -> str:
    try:
        if ts is None:
            ts = time.time()
        return datetime.utcfromtimestamp(float(ts)).isoformat() + 'Z'
    except Exception:
        return ''


def _append_run_result(result: dict) -> None:
    try:
        with open(RUN_RESULTS_FILE, 'a', encoding='utf-8') as f:
            f.write(json.dumps(result, ensure_ascii=False) + "\n")
    except Exception:
        pass


def _reset_per_run_artifacts() -> None:
    """清理每次 run 的輸出檔案，確保重複測試不互相污染。

    注意：不刪除 STATE_FILE（含隊列/狀態）與 RUN_RESULTS_FILE（歷史結果）。
    """
    files = [
        'primes_reports.ndjson',
        'progress_reports.ndjson',
        'primes_result.json',
        # 舊版可能存在的檔案
        'primes_partial.ndjson',
    ]
    for p in files:
        try:
            if os.path.exists(p):
                os.remove(p)
        except Exception:
            pass


def _start_next_run_locked() -> bool:
    """If there is a queued run and nothing is currently running, start it. lock must be held."""
    global current_run, _run_seq
    global max_active_workers
    global num_chunks, task_running, start_time

    # 還在跑或還有工作就不動
    if bool(task_running) or bool(in_flight) or bool(remaining_chunks):
        return False

    if not job_queue:
        current_run = None
        _save_state()
        return False

    cfg = job_queue.pop(0)
    _save_queue()
    try:
        n = int(cfg.get('n'))
    except Exception:
        n = None
    try:
        chunks = int(cfg.get('chunks') or 1)
    except Exception:
        chunks = 1
    try:
        desired_workers = cfg.get('desired_workers', None)
        desired_workers = int(desired_workers) if desired_workers is not None else None
    except Exception:
        desired_workers = None

    try:
        rep_index = int(cfg.get('rep_index') or 0)
    except Exception:
        rep_index = 0
    try:
        rep_total = int(cfg.get('rep_total') or 0)
    except Exception:
        rep_total = 0

    if not n or n <= 0:
        return False

    _run_seq = int(_run_seq or 0) + 1
    run_id = f"run:{_run_seq}:{n}:{chunks}"

    # 套用本次 run 的 worker 上限（None/0 = 不限制）
    if desired_workers is not None and desired_workers > 0:
        max_active_workers = int(desired_workers)

    # fresh start for each queued run（每次都重製）
    _reset_per_run_artifacts()
    init_server_from_params(n)
    remaining_chunks[:] = _build_numeric_tasks(power_n, chunks=chunks, align=1000)
    num_chunks = len(remaining_chunks)
    task_running = True
    start_time = None

    current_run = {
        'run_id': run_id,
        'n': int(n),
        'chunks': int(num_chunks),
        'desired_workers': desired_workers,
        'rep_index': rep_index if rep_index > 0 else None,
        'rep_total': rep_total if rep_total > 0 else None,
        'enqueued_at': cfg.get('enqueued_at'),
        'started_at': _utc_iso(),
    }

    _save_state()
    return True

@app.route('/')
def index():
    return render_template('index.html')

@app.route('/start_task', methods=['POST'])
def start_task():
    global task_running, task_initialized
    global power_n, num_chunks, remaining_chunks
    global in_flight, completed_task_ids, task_cursors
    global tasks_completed, start_time, durations
    global total_numbers, processed_numbers, buffered_primes_count, max_prime_seen
    data = request.json or {}
    n = data.get('n')
    chunks = data.get('chunks', 1)
    resume_raw = data.get('resume', None)
    reset = bool(data.get('reset', False))
    
    if not n:
        return jsonify({'error': 'Parameter n is required'}), 400
    
    try:
        n = int(n)
        if n <= 0:
            return jsonify({'error': 'n must be a positive integer'}), 400
    except ValueError:
        return jsonify({'error': 'n must be a valid integer'}), 400
    
    with lock:
        if task_running and task_initialized:
            # 避免 UI 連點或重整造成的「已在跑」誤判：同一個 n 直接回傳目前狀態，讓前端繼續監看
            try:
                current_n = int(power_n) if power_n is not None else None
            except Exception:
                current_n = None

            if reset:
                return jsonify({'error': 'Task is running; cannot reset while running'}), 409

            if current_n is not None and int(n) == current_n:
                return jsonify({
                    'status': 'already_running',
                    'power_n': current_n,
                    'chunks': _num_chunks_int(),
                    'completed_chunks': tasks_completed,
                    'remaining_chunks': len(remaining_chunks),
                    'in_flight': len(in_flight),
                    'processed_numbers': processed_numbers,
                    'total_numbers': total_numbers,
                })

            return jsonify({
                'error': 'Task already running',
                'running_power_n': current_n,
            }), 409

    with lock:
        # 若未指定 resume，且 state 存在且 n 相符，預設自動續算
        if resume_raw is None:
            resume = (not reset) and (_peek_state_power_n() == int(n))
        else:
            resume = bool(resume_raw)

        if (not reset) and resume and _load_state() and int(power_n or 0) == int(n):
            sane, reason = _state_sane_for_power(int(n))
            if not sane:
                return jsonify({
                    'error': 'State file exists but looks inconsistent; please use reset to restart cleanly.',
                    'state_reason': reason,
                }), 409
            # resume：把超時的 in-flight 任務丟回去
            _requeue_expired_inflight(time.time())
            # 若還有未完成工作，確保 running=true（避免 state 殘留造成 UI 誤判完成）
            try:
                chunks_loaded = int(_num_chunks_int())
                if chunks_loaded > 0 and int(tasks_completed or 0) < chunks_loaded and (remaining_chunks or in_flight):
                    task_running = True
            except Exception:
                pass
            _save_state()
            return jsonify({
                'status': 'resumed',
                'power_n': power_n,
                'chunks': _num_chunks_int(),
                'completed_chunks': tasks_completed,
                'remaining_chunks': len(remaining_chunks),
            })

        # fresh start
        try:
            chunks = int(chunks)
            if chunks <= 0:
                chunks = 1
        except Exception:
            chunks = 1

        init_server_from_params(n)
        # 依使用者指定 chunks 重建任務
        remaining_chunks[:] = _build_numeric_tasks(power_n, chunks=chunks, align=1000)
        num_chunks = len(remaining_chunks)
        in_flight.clear()
        completed_task_ids.clear()
        task_cursors.clear()

        # fresh start：計時從實際派發任務開始
        start_time = None
        task_running = True

        # start_task 視為手動單次 run：清掉 current_run 但不動 queue
        global current_run
        current_run = {
            'run_id': f"manual:{int(time.time())}:{int(n)}:{int(num_chunks)}",
            'n': int(n),
            'chunks': int(num_chunks),
            'desired_workers': None,
            'enqueued_at': None,
            'started_at': _utc_iso(),
        }

        # reset 時清理舊檔案；未 reset 則保留，避免誤刪你想續用的資料
        if reset:
            for p in ['primes_reports.ndjson', 'progress_reports.ndjson', 'primes_result.json', str(STATE_FILE)]:
                try:
                    if os.path.exists(p):
                        os.remove(p)
                except Exception:
                    pass

        _save_state()
    
    return jsonify({
        'status': 'started',
        'power_n': n,
        'chunks': _num_chunks_int(),
        'description': f'Range 1 to 10^{n}'
    })


@app.route('/enqueue_runs', methods=['POST'])
def enqueue_runs():
    """加入任務隊列：可指定 repeats 次數與限制參與 worker 台數。"""
    data = request.json or {}
    try:
        n = int(data.get('n'))
    except Exception:
        return jsonify({'error': 'n must be a valid integer'}), 400

    try:
        chunks = int(data.get('chunks') or 1)
        if chunks <= 0:
            chunks = 1
    except Exception:
        chunks = 1

    try:
        repeats = int(data.get('repeats') or 1)
        if repeats <= 0:
            repeats = 1
    except Exception:
        repeats = 1

    desired_workers_raw = data.get('desired_workers', None)
    if desired_workers_raw in (None, '', 0, '0'):
        desired_workers = None
    else:
        try:
            desired_workers = int(desired_workers_raw)
            if desired_workers <= 0:
                desired_workers = None
        except Exception:
            desired_workers = None

    with lock:
        ts = _utc_iso()
        for i in range(repeats):
            job_queue.append({
                'n': int(n),
                'chunks': int(chunks),
                'desired_workers': desired_workers,
                'enqueued_at': ts,
                'rep_index': int(i + 1),
                'rep_total': int(repeats),
            })

        _save_queue()

        started = False
        if (not task_running) and (not in_flight) and (not remaining_chunks):
            started = _start_next_run_locked()

        _save_state()

    return jsonify({
        'status': 'queued',
        'queued': repeats,
        'queue_length': len(job_queue),
        'started_now': bool(started),
        'desired_workers': desired_workers,
    })


@app.route('/enqueue_experiment_grid', methods=['POST'])
def enqueue_experiment_grid():
    """批次加入實驗組合。

    body:
      {
        "n_values": [5,6,7],
        "worker_values": [1,10,20],
        "chunk_values": [50,100],
        "repeats": 50,
        "clear_queue": false
      }
    """
    data = request.json or {}
    n_values = data.get('n_values') or []
    worker_values = data.get('worker_values') or []
    chunk_values = data.get('chunk_values') or []
    repeats = data.get('repeats') or 1
    clear_queue = bool(data.get('clear_queue', False))

    try:
        repeats = int(repeats)
        if repeats <= 0:
            repeats = 1
    except Exception:
        repeats = 1

    def _to_int_list(v):
        out = []
        if not isinstance(v, list):
            return out
        for x in v:
            try:
                out.append(int(x))
            except Exception:
                continue
        return out

    n_values = _to_int_list(n_values)
    worker_values = _to_int_list(worker_values)
    chunk_values = _to_int_list(chunk_values)

    n_values = [n for n in n_values if n > 0]
    worker_values = [w for w in worker_values if w > 0]
    chunk_values = [c for c in chunk_values if c > 0]

    if not n_values or not worker_values or not chunk_values:
        return jsonify({'error': 'n_values/worker_values/chunk_values are required and must be non-empty'}), 400

    # 防呆：避免一次 enqueue 超大把記憶體打爆
    combos = len(n_values) * len(worker_values) * len(chunk_values)
    total_to_enqueue = combos * repeats
    if total_to_enqueue > 200000:
        return jsonify({'error': 'Too many runs requested', 'total': total_to_enqueue}), 413

    with lock:
        if clear_queue:
            job_queue.clear()
        ts = _utc_iso()

        enqueued = 0
        for n in n_values:
            for w in worker_values:
                for c in chunk_values:
                    for i in range(repeats):
                        job_queue.append({
                            'n': int(n),
                            'chunks': int(c),
                            'desired_workers': int(w),
                            'enqueued_at': ts,
                            'rep_index': int(i + 1),
                            'rep_total': int(repeats),
                        })
                        enqueued += 1

        _save_queue()

        started = False
        if (not task_running) and (not in_flight) and (not remaining_chunks):
            started = _start_next_run_locked()

        _save_state()

    return jsonify({
        'status': 'queued',
        'enqueued': enqueued,
        'combos': combos,
        'repeats': repeats,
        'queue_length': len(job_queue),
        'started_now': bool(started),
        'n_values': n_values,
        'worker_values': worker_values,
        'chunk_values': chunk_values,
    })


@app.route('/queue_status', methods=['GET'])
def queue_status():
    with lock:
        chunks = _num_chunks_int()
        effective_running = bool(task_running) or bool(in_flight) or bool(remaining_chunks)
        limit = max_active_workers
        return jsonify({
            'running': bool(effective_running),
            'current_run': current_run,
            'queue_length': len(job_queue),
            'active_workers': len(active_workers),
            'max_active_workers': limit,
            'power_n': power_n,
            'total_chunks': chunks,
            'completed_chunks': tasks_completed,
            'remaining_chunks': len(remaining_chunks),
            'in_flight': len(in_flight),
            'total_primes_found': buffered_primes_count,
            'elapsed_time': round((time.time() - start_time) if start_time else 0, 2),
        })


@app.route('/results_tail', methods=['GET'])
def results_tail():
    """回傳最後 N 筆結果（NDJSON）。"""
    try:
        n = int(request.args.get('n') or 20)
        if n <= 0:
            n = 20
        n = min(n, 200)
    except Exception:
        n = 20

    items = []
    try:
        if RUN_RESULTS_FILE.exists():
            with open(RUN_RESULTS_FILE, 'r', encoding='utf-8') as f:
                lines = f.readlines()[-n:]
            for line in lines:
                line = (line or '').strip()
                if not line:
                    continue
                try:
                    items.append(json.loads(line))
                except Exception:
                    continue
    except Exception:
        pass
    return jsonify({'items': items})


@app.route('/task_state', methods=['GET'])
def task_state():
    with lock:
        inflight_preview = []
        try:
            for task_id, info in list(in_flight.items())[:5]:
                task = (info or {}).get('task') or {}
                inflight_preview.append({
                    'task_id': task_id,
                    'worker_id': (info or {}).get('worker_id'),
                    'assigned_at': (info or {}).get('assigned_at'),
                    'start': task.get('start'),
                    'end': task.get('end'),
                    'cursor': task_cursors.get(task_id),
                })
        except Exception:
            inflight_preview = []

        sane, sane_reason = _state_sane_for_power(int(power_n) if power_n is not None else 0)
        expected_end = _expected_range_end(int(power_n) if power_n is not None else 0)
        max_end = _max_task_end_from_state()

        effective_running = bool(task_running) or bool(in_flight) or bool(remaining_chunks)

        return jsonify({
            'has_state_file': bool(STATE_FILE.exists()),
            'initialized': bool(task_initialized),
            'running': bool(effective_running),
            'power_n': power_n,
            'total_chunks': _num_chunks_int(),
            'completed_chunks': tasks_completed,
            'remaining_chunks': len(remaining_chunks),
            'in_flight': len(in_flight),
            'in_flight_preview': inflight_preview,
            'expected_end': expected_end,
            'max_task_end': max_end,
            'state_sane': bool(sane),
            'state_sane_reason': sane_reason,
            'processed_numbers': processed_numbers,
            'total_numbers': total_numbers,
            'total_primes_found': buffered_primes_count,
        })

@app.route('/progress', methods=['GET'])
def get_progress():
    with lock:
        if not task_initialized:
            return jsonify({'initialized': False})

        chunks = _num_chunks_int()

        if total_numbers > 0:
            progress_percent = (processed_numbers / total_numbers * 100)
        else:
            progress_percent = (tasks_completed / chunks * 100) if chunks > 0 else 0
        elapsed_time = time.time() - start_time if start_time else 0

        # 避免因 state/計數不一致導致 UI 提早顯示完成：只要還有 in-flight/remaining 就視為 running
        effective_running = bool(task_running) or bool(in_flight) or bool(remaining_chunks)
        try:
            if chunks > 0 and int(tasks_completed or 0) < int(chunks):
                effective_running = True
        except Exception:
            pass
        
        return jsonify({
            'initialized': True,
            'running': bool(effective_running),
            'total_chunks': chunks,
            'completed_chunks': tasks_completed,
            'remaining_chunks': len(remaining_chunks),
            'progress_percent': round(progress_percent, 2),
            'elapsed_time': round(elapsed_time, 2),
            'power_n': power_n,
            'total_primes_found': buffered_primes_count,
            'processed_numbers': processed_numbers,
            'total_numbers': total_numbers,
        })

@app.route('/report_progress', methods=['POST'])
def report_progress():
    """Worker 回報任務進度（不傳送實際質數）"""
    global tasks_completed, durations, buffered_primes_count, processed_numbers
    data = request.json or {}
    primes_count_raw = data.get('primes_count', 0)
    primes_count_delta_raw = data.get('primes_count_delta', None)
    numbers_processed_delta_raw = data.get('numbers_processed_delta', None)
    duration_raw = data.get('duration')
    worker_id = data.get('worker_id', 'unknown')
    range_start = data.get('range_start', 0)
    range_end = data.get('range_end', 0)
    task_progress = data.get('task_progress', 100.0)  # 該任務的完成百分比
    result_file = data.get('result_file')
    meta = data.get('meta')
    done = data.get('done', None)
    primes_list = data.get('primes')

    # 向下相容：
    # - 舊 client 送 primes_count（一次性結果）
    # - 新 client 送 primes_count_delta / numbers_processed_delta（增量）
    if primes_count_delta_raw is None:
        try:
            primes_count_delta = int(primes_count_raw)
        except Exception:
            primes_count_delta = 0
    else:
        try:
            primes_count_delta = int(primes_count_delta_raw)
        except Exception:
            primes_count_delta = 0

    # 若 client 直接送 primes list（例如每 1000 個數字一次），優先用 list 長度
    if isinstance(primes_list, list):
        try:
            primes_count_delta = int(len(primes_list))
        except Exception:
            pass

    if numbers_processed_delta_raw is None:
        numbers_processed_delta = 0
    else:
        try:
            numbers_processed_delta = int(numbers_processed_delta_raw)
        except Exception:
            numbers_processed_delta = 0

    try:
        duration = float(duration_raw) if duration_raw is not None else None
    except Exception:
        duration = None

    with lock:
        global start_time
        if start_time is None:
            start_time = time.time()

        chunks = _num_chunks_int()
        # 若有 primes list，存成每行一個 JSON array： [2,3,5,7]
        if isinstance(primes_list, list) and primes_list:
            try:
                with open('primes_reports.ndjson', 'a', encoding='utf-8') as f:
                    f.write(json.dumps(primes_list, ensure_ascii=False) + "\n")
            except Exception:
                pass

        # 伺服器端保存每次回報（含計算量）
        try:
            report_line = {
                'ts': time.time(),
                'worker_id': worker_id,
                'range_start': range_start,
                'range_end': range_end,
                'primes_count_delta': primes_count_delta,
                'numbers_processed_delta': numbers_processed_delta,
                'duration': duration,
                'task_progress': task_progress,
                'result_file': result_file,
                'meta': meta,
                'done': done,
            }
            with open('progress_reports.ndjson', 'a', encoding='utf-8') as f:
                f.write(json.dumps(report_line, ensure_ascii=False) + "\n")
        except Exception:
            pass

        # 累計質數數量
        buffered_primes_count += primes_count_delta
        processed_numbers += numbers_processed_delta

        if duration is not None:
            durations.append(duration)

        # done 未提供時，視為舊版一次性回報 -> 等同完成
        if done is None:
            done = True

        if done:
            tasks_completed += 1

        progress_percent = (tasks_completed / chunks * 100) if chunks > 0 else 0
        
        # 顯示詳細進度資訊
        duration_str = f"{duration:.2f}s" if duration is not None else "n/a"
        extra = f" file={result_file}" if result_file else ""
        if total_numbers > 0:
            overall_percent = (processed_numbers / total_numbers) * 100
            overall_str = f"{processed_numbers:,}/{total_numbers:,} ({overall_percent:.1f}%)"
        else:
            overall_str = f"{tasks_completed}/{chunks} ({progress_percent:.1f}%)"

        print(
            f"[{worker_id}] +{numbers_processed_delta:,} numbers, +{primes_count_delta:,} primes, "
            f"{duration_str}, 任務完成度 {float(task_progress):.1f}% "
            f"| 總進度: {overall_str}" + extra
        )

        # 檢查是否全部完成（以 chunk 完成數判斷）
        if chunks > 0 and tasks_completed >= chunks:
            total_time = time.time() - (start_time or time.time())
            
            print(f"\n{'='*70}")
            print(f"所有任務已完成！")
            print(f"找到質數總數: {buffered_primes_count:,}")
            print(f"總耗時: {total_time:.2f}s")
            if durations:
                print(f"平均每任務: {sum(durations)/len(durations):.2f}s")
            print(f"{'='*70}\n")
            
            # 保存統計結果
            result_data = {
                'total_count': buffered_primes_count,
                'total_time': total_time,
                'average_chunk_time': sum(durations)/len(durations) if durations else 0,
                'client_durations': durations,
                'power_n': power_n,
                'num_chunks': num_chunks,
                'note': 'Progress-based reporting: primes saved locally on each worker in {start}-{end}.json format'
            }
            
            out_path = 'primes_result.json'
            try:
                with open(out_path, 'w', encoding='utf-8') as f:
                    json.dump(result_data, f, ensure_ascii=False, indent=2)
            except Exception as e:
                print(f"Failed to save result: {e}")
            
            global task_running
            task_running = False

    return jsonify({
        'status': 'success',
        'tasks_completed': tasks_completed,
        'total_chunks': chunks,
        'progress_percent': round(progress_percent, 2),
        'total_primes': buffered_primes_count,
        'processed_numbers': processed_numbers,
        'total_numbers': total_numbers,
    })


@app.route('/report_progress_batch', methods=['POST'])
def report_progress_batch():
    """批次回報：items 內每筆對應一個 1000-window 的 primes list，伺服器存成每行一個 JSON array。"""
    global tasks_completed, durations, buffered_primes_count, processed_numbers, max_prime_seen
    data = request.json or {}
    worker_id = data.get('worker_id', 'unknown')
    task_id = data.get('task_id')
    items = data.get('items') or []
    done = data.get('done', False)
    duration_raw = data.get('duration')
    meta = data.get('meta')
    range_start = data.get('range_start', 0)
    range_end = data.get('range_end', 0)
    store_primes = bool(data.get('store_primes', True))
    store_progress = bool(data.get('store_progress', True))

    try:
        duration = float(duration_raw) if duration_raw is not None else None
    except Exception:
        duration = None

    primes_delta = 0
    numbers_delta = 0

    with lock:
        global start_time
        if start_time is None:
            start_time = time.time()

        chunks = _num_chunks_int()
        # 去重：done=true 的 task 只計一次
        if task_id and bool(done) and task_id in completed_task_ids:
            return jsonify({'status': 'duplicate_done_ignored', 'task_id': task_id})

        # per-task 去重游標：避免重送同一批/同一 window 造成 primes_reports 重複寫入
        if task_id:
            try:
                cursor = int(task_cursors.get(task_id) or int(range_start) or 0)
            except Exception:
                cursor = int(range_start) if isinstance(range_start, int) else 0
        else:
            cursor = None

        # heartbeat：收到該 task 的回報就延長租約，避免長任務被錯誤回收
        if task_id:
            try:
                info = in_flight.get(task_id) or {}
                if (not info) or (str(info.get('worker_id')) == str(worker_id)):
                    in_flight[task_id] = {
                        'assigned_at': time.time(),
                        'worker_id': worker_id,
                        'task': info.get('task') or {'task_id': task_id, 'start': range_start, 'end': range_end},
                    }
            except Exception:
                pass

        # 防呆：確保 window 順序遞增，避免 cursor 因順序錯亂而跳過較早 window
        try:
            if isinstance(items, list):
                items = sorted(items, key=lambda it: int((it or {}).get('window_end') or 0))
        except Exception:
            pass

        for item in items:
            if not isinstance(item, dict):
                continue

            try:
                window_end = int(item.get('window_end'))
            except Exception:
                window_end = None
            try:
                window_start = int(item.get('window_start'))
            except Exception:
                window_start = None

            # 若已回報到 cursor（exclusive），則忽略重複 window，避免重複計數與重複寫檔
            if cursor is not None and window_end is not None and window_end <= cursor:
                continue

            primes = item.get('primes')
            if not isinstance(primes, list):
                primes = []

            # 可選：由 worker 直接傳增量，避免一定要帶 primes list
            try:
                primes_count_delta = item.get('primes_count_delta', None)
                primes_count_delta = int(primes_count_delta) if primes_count_delta is not None else None
            except Exception:
                primes_count_delta = None

            try:
                item_max_prime = item.get('max_prime', None)
                item_max_prime = int(item_max_prime) if item_max_prime is not None else None
            except Exception:
                item_max_prime = None

            # 存成 [2,3,5,7]\n
            if store_primes and primes:
                try:
                    _primes_lines_buffer.append(json.dumps(primes, ensure_ascii=False) + "\n")
                except Exception:
                    pass

            if primes_count_delta is None:
                primes_count_delta = len(primes)
            primes_delta += int(primes_count_delta or 0)

            if item_max_prime is None and primes:
                try:
                    item_max_prime = int(primes[-1])
                except Exception:
                    item_max_prime = None

            if item_max_prime is not None:
                try:
                    if max_prime_seen is None:
                        max_prime_seen = int(item_max_prime)
                    else:
                        max_prime_seen = max(int(max_prime_seen), int(item_max_prime))
                except Exception:
                    pass

            # numbers_processed_delta 以 item 內為準；若沒有就從 window 計算
            try:
                nd = item.get('numbers_processed_delta', None)
                if nd is None and (window_start is not None and window_end is not None):
                    nd = int(window_end - window_start)
                numbers_delta += int(nd or 0)
            except Exception:
                pass

            if cursor is not None and window_end is not None:
                try:
                    cursor = max(int(cursor), int(window_end))
                except Exception:
                    pass

        if task_id and cursor is not None:
            task_cursors[task_id] = int(cursor)

        buffered_primes_count += primes_delta
        processed_numbers += numbers_delta

        try:
            _flush_primes_lines(force=False)
        except Exception:
            pass

        # 紀錄一筆 batch summary（避免每 1000-window 一筆造成 progress_reports 太大）
        if store_progress:
            try:
                report_line = {
                    'ts': time.time(),
                    'worker_id': worker_id,
                    'range_start': range_start,
                    'range_end': range_end,
                    'items': len(items),
                    'numbers_processed_delta': numbers_delta,
                    'primes_count_delta': primes_delta,
                    'max_prime_seen': max_prime_seen,
                    'duration': duration,
                    'done': bool(done),
                    'meta': meta,
                }
                with open('progress_reports.ndjson', 'a', encoding='utf-8') as f:
                    f.write(json.dumps(report_line, ensure_ascii=False) + "\n")
            except Exception:
                pass

        if duration is not None:
            durations.append(duration)

        if done:
            if task_id:
                completed_task_ids.add(task_id)
                # done 時把游標推到 range_end，代表這個 task 完成
                try:
                    task_cursors[task_id] = max(int(task_cursors.get(task_id) or 0), int(range_end))
                except Exception:
                    pass
                in_flight.pop(task_id, None)
            tasks_completed += 1
            # done 時強制 flush
            try:
                _flush_primes_lines(force=True)
            except Exception:
                pass

            _save_state()

            if chunks > 0 and tasks_completed >= chunks:
                total_time = time.time() - (start_time or time.time())
                print(f"\n{'='*70}")
                print("所有任務已完成！")
                print(f"找到質數總數: {buffered_primes_count:,}")
                if max_prime_seen is not None:
                    print(f"最大質數: {max_prime_seen:,}")
                print(f"總耗時: {total_time:.2f}s")
                if durations:
                    print(f"平均每回報批次: {sum(durations)/len(durations):.2f}s")
                print(f"{'='*70}\n")
                try:
                    result_data = {
                        'total_count': buffered_primes_count,
                        'max_prime': max_prime_seen,
                        'processed_numbers': processed_numbers,
                        'total_numbers': total_numbers,
                        'total_time': total_time,
                        'power_n': power_n,
                        'num_chunks': chunks,
                        'note': 'Progress-based reporting; primes may be discarded server-side when store_primes=false.'
                    }
                    with open('primes_result.json', 'w', encoding='utf-8') as f:
                        json.dump(result_data, f, ensure_ascii=False, indent=2)
                except Exception:
                    pass

                global task_running
                task_running = False

                # 每個 run 追加保存總數與耗時
                try:
                    run_info = current_run or {}
                    _append_run_result({
                        'ts': _utc_iso(),
                        'run_id': run_info.get('run_id'),
                        'n': run_info.get('n', power_n),
                        'chunks': run_info.get('chunks', chunks),
                        'desired_workers': run_info.get('desired_workers'),
                        'rep_index': run_info.get('rep_index'),
                        'rep_total': run_info.get('rep_total'),
                        'total_primes': int(buffered_primes_count or 0),
                        'processed_numbers': int(processed_numbers or 0),
                        'total_numbers': int(total_numbers or 0),
                        'total_time_sec': float(total_time),
                        'max_prime': max_prime_seen,
                    })
                except Exception:
                    pass

                # 自動啟動下一個 queued run
                try:
                    _start_next_run_locked()
                except Exception:
                    pass

        # 週期性保存 state
        try:
            _save_state()
        except Exception:
            pass

    return jsonify({
        'status': 'success',
        'items': len(items),
        'processed_numbers': processed_numbers,
        'total_numbers': total_numbers,
        'total_primes': buffered_primes_count,
        'done': bool(done),
    })

@app.route('/get_task', methods=['GET'])
def get_task():
    global tasks_completed
    worker_id = request.args.get('worker_id', 'unknown')
    take_over = str(request.args.get('take_over', '') or '').lower() in ('1', 'true', 'yes', 'y')
    now_ts = time.time()
    with lock:
        global start_time

        # 控制可參與運算的 worker 台數（0/None=不限制）
        limit = max_active_workers
        if limit is not None:
            try:
                limit = int(limit)
            except Exception:
                limit = None

        if limit is not None and limit > 0:
            if worker_id not in active_workers:
                if len(active_workers) >= limit:
                    # 不允許加入（但維持 204，避免 client crash）
                    return jsonify({'message': 'Worker limit reached'}), 204
                active_workers.add(str(worker_id))
        # 先把目前 worker 已領取但尚未完成的 in-flight 任務回傳（支援 worker 重啟後接續）
        try:
            for tid, info in list(in_flight.items()):
                if str(info.get('worker_id')) != str(worker_id):
                    continue
                task = (info.get('task') or {})
                task_id = task.get('task_id') or tid
                if task_id and task_id in completed_task_ids:
                    in_flight.pop(tid, None)
                    continue
                if task_id and task_id in task_cursors:
                    try:
                        cursor = int(task_cursors.get(task_id) or 0)
                        start_v = int(task.get('start') or 0)
                        end_v = int(task.get('end') or 0)
                        if cursor > start_v:
                            task = dict(task)
                            task['start'] = cursor
                        if cursor >= end_v:
                            completed_task_ids.add(task_id)
                            in_flight.pop(tid, None)
                            try:
                                tasks_completed = int(tasks_completed or 0) + 1
                            except Exception:
                                tasks_completed = tasks_completed + 1
                            _save_state()
                            continue
                    except Exception:
                        pass
                # 刷新 assigned_at，避免剛重連就被回收
                try:
                    info['assigned_at'] = now_ts
                    in_flight[tid] = info
                except Exception:
                    pass
                if start_time is None:
                    start_time = now_ts
                _save_state()
                return jsonify(task)
        except Exception:
            pass

        _requeue_expired_inflight(now_ts)

        # 可選：接手最舊的 in-flight（用於 chunks=1 但原 worker 掛掉、想立即換機續跑）
        if take_over and not remaining_chunks and in_flight:
            try:
                # 找最舊的 in-flight
                tid, info = min(in_flight.items(), key=lambda kv: float((kv[1] or {}).get('assigned_at') or 0))
                task = (info.get('task') or {})
                task_id = task.get('task_id') or tid
                if task_id and task_id in completed_task_ids:
                    in_flight.pop(tid, None)
                else:
                    # 重新指派給此 worker
                    in_flight[tid] = {
                        'assigned_at': now_ts,
                        'worker_id': worker_id,
                        'task': task,
                    }
                    # cursor 續算
                    if task_id and task_id in task_cursors:
                        try:
                            cursor = int(task_cursors.get(task_id) or 0)
                            if cursor > int(task.get('start') or 0):
                                task = dict(task)
                                task['start'] = cursor
                        except Exception:
                            pass
                    _save_state()
                    return jsonify(task)
            except Exception:
                pass
        while remaining_chunks:
            task = remaining_chunks.pop(0)
            task_id = task.get('task_id')
            if task_id and task_id in completed_task_ids:
                continue

            # 若此 task 曾回報到某個 window_end，重派時從 cursor 續算（避免單一大區塊重跑已完成部分）
            if task_id and task_id in task_cursors:
                try:
                    cursor = int(task_cursors.get(task_id) or 0)
                    start_v = int(task.get('start') or 0)
                    end_v = int(task.get('end') or 0)
                    if cursor > start_v:
                        task = dict(task)
                        task['start'] = cursor
                    # 若 cursor 已到 end，視為完成（避免最後一段 done 遺失導致卡住）
                    if cursor >= end_v and task_id not in completed_task_ids:
                        completed_task_ids.add(task_id)
                        try:
                            tasks_completed = int(tasks_completed or 0) + 1
                        except Exception:
                            tasks_completed = tasks_completed + 1
                        _save_state()
                        continue
                except Exception:
                    pass
            if task_id:
                in_flight[task_id] = {
                    'assigned_at': now_ts,
                    'worker_id': worker_id,
                    'task': task,
                }
            if start_time is None:
                start_time = now_ts
            _save_state()
            return jsonify(task)
        return jsonify({'message': 'No more tasks'}), 204


# 嘗試在啟動時自動載入狀態（讓你重啟 Flask 後可直接續算）
try:
    _load_state()
except Exception:
    pass

# 若 state 沒載到，至少嘗試載入 queue
try:
    if not job_queue:
        _load_queue()
except Exception:
    pass

@app.route('/report_result', methods=['POST'])
def report_result():
    """接收 worker 的最終結果（向下兼容舊版）"""
    global tasks_completed, start_time, durations, buffered_primes, buffered_primes_count, task_running
    data = request.json or {}
    primes = data.get('primes', [])
    primes_count = data.get('primes_count', len(primes))
    duration = data.get('duration')
    worker_id = data.get('worker_id', 'unknown')

    with lock:
        global start_time
        if start_time is None:
            start_time = time.time()

        # 只統計數量，不保存實際質數（節省網路和記憶體）
        if SAVE_ACTUAL_PRIMES and primes:
            try:
                buffered_primes.extend(int(p) for p in primes)
            except Exception:
                pass
        
        # 累計質數數量
        buffered_primes_count += primes_count

        if duration:
            try:
                durations.append(float(duration))
            except Exception:
                pass

        tasks_completed += 1

        print(f"Result from {worker_id}: {primes_count:,} primes, duration={duration:.2f}s ({tasks_completed}/{num_chunks})")

        # 在緩衝達到閾值時逐步寫入磁碟（僅在保存實際質數時）
        if SAVE_ACTUAL_PRIMES:
            try:
                _flush_buffer_to_disk()
            except Exception as e:
                print(f"Flush error: {e}")

        if tasks_completed == num_chunks:
            # 強制 flush 所有緩衝到磁碟
            if SAVE_ACTUAL_PRIMES:
                _flush_buffer_to_disk(force=True)

            total_time = time.time() - (start_time or time.time())

            # 處理結果
            if SAVE_ACTUAL_PRIMES:
                # 從追加檔案讀取所有質數
                collected = []
                try:
                    with open(APPEND_FILE, 'r', encoding='utf-8') as f:
                        for line in f:
                            line = line.strip()
                            if not line:
                                continue
                            try:
                                collected.append(int(line))
                            except Exception:
                                continue
                except FileNotFoundError:
                    pass

                # 若緩衝區中仍有未 flush 的也加入
                if buffered_primes:
                    collected.extend(buffered_primes)

                unique_primes = sorted(set(collected))
                result_data = {
                    'primes': unique_primes, 
                    'total_count': len(unique_primes), 
                    'total_time': total_time, 
                    'client_durations': durations
                }
            else:
                # 只保存統計資訊
                result_data = {
                    'total_count': buffered_primes_count,
                    'total_time': total_time,
                    'client_durations': durations,
                    'power_n': power_n,
                    'note': 'Only count saved, not actual primes (for large scale computation)'
                }

            out_path = 'primes_result.json'
            try:
                with open(out_path, 'w', encoding='utf-8') as f:
                    json.dump(result_data, f, ensure_ascii=False, indent=2)
                print(f"All tasks completed. Found {buffered_primes_count:,} primes in total")
                print(f"Total elapsed time: {total_time:.2f}s")
            except Exception as e:
                print(f"Failed to save result: {e}")
            
            # Mark task as completed
            task_running = False

    return jsonify({'status': 'success', 'received': primes_count})

if __name__ == '__main__':
    app.run(host='0.0.0.0', port=5001, threaded=True, debug=False)