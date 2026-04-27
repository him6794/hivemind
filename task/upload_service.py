from __future__ import annotations

import os
import json
from pathlib import Path
from typing import Optional

import requests
from fastapi import FastAPI, File, Form, UploadFile


app = FastAPI(title="HiveMind Upload Service")


def _safe_filename(name: str) -> str:
    name = os.path.basename(name).strip().replace("\\x00", "")
    if not name:
        return "result.json"
    return name


@app.get("/health")
def health():
    return {"status": "ok"}


@app.post("/upload_result")
def upload_result(
    file: UploadFile = File(...),
    worker_id: str = Form("unknown"),
    range_start: int = Form(0),
    range_end: int = Form(0),
    primes_count: int = Form(0),
    duration: float = Form(0.0),
    meta_json: str = Form("{}"),
    notify_url: str = Form(""),
    # 向下相容：舊 client 仍會送 forward_url，我們把它當成 notify_url
    forward_url: str = Form(""),
) -> dict:
    uploads_dir = Path(__file__).resolve().parent / "uploads"
    uploads_dir.mkdir(parents=True, exist_ok=True)

    safe_name = _safe_filename(file.filename or "result.json")
    out_name = f"{worker_id}_{range_start}-{range_end}_{safe_name}"
    out_path = uploads_dir / out_name

    with out_path.open("wb") as f:
        while True:
            chunk = file.file.read(1024 * 1024)
            if not chunk:
                break
            f.write(chunk)

    try:
        meta = json.loads(meta_json) if meta_json else {}
        if not isinstance(meta, dict):
            meta = {"value": meta}
    except Exception:
        meta = {"raw": meta_json}

    # 儲存完才通知 Flask（不轉送檔案內容）
    effective_notify_url = (notify_url or forward_url or "").strip()
    notify_status: Optional[int] = None
    notify_body: Optional[str] = None
    try:
        if effective_notify_url:
            payload = {
                "worker_id": worker_id,
                "range_start": range_start,
                "range_end": range_end,
                "primes_count": primes_count,
                "duration": duration,
                "task_progress": 100.0,
                "result_file": str(out_path),
                "meta": meta,
            }
            r = requests.post(effective_notify_url, json=payload, timeout=15)
            notify_status = r.status_code
            notify_body = r.text[:2000]
    except Exception as e:
        notify_body = f"notify_failed: {e}"

    return {
        "saved_to": str(out_path),
        "notify_url": effective_notify_url or None,
        "notify_status": notify_status,
        "notify_body": notify_body,
    }
