"""User session helpers for WorkerNode."""
from __future__ import annotations

from uuid import uuid4
from datetime import datetime


def create_user_session(node, username: str, token: str) -> str:
    session_id = str(uuid4())
    session_data = {
        'username': username,
        'token': token,
        'login_time': datetime.now(),
        'cpt_balance': 0,
        'created_at': node.time() if hasattr(node, 'time') else __import__('time').time(),
    }
    with node.session_lock:
        node.user_sessions[session_id] = session_data
    return session_id


def get_user_session(node, session_id: str):
    with node.session_lock:
        return node.user_sessions.get(session_id)


def update_session_balance(node, session_id: str, balance: int) -> None:
    with node.session_lock:
        if session_id in node.user_sessions:
            node.user_sessions[session_id]['cpt_balance'] = balance


def clear_user_session(node, session_id: str) -> None:
    with node.session_lock:
        if session_id in node.user_sessions:
            del node.user_sessions[session_id]
