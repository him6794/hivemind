"""Flask web application setup and routes for WorkerNode.

This module registers routes on a given Flask app and starts the server
in a background thread. All state and actions are delegated to the
provided `worker` instance to avoid circular dependencies.
"""

from __future__ import annotations

import time
import logging
from datetime import datetime, timedelta
from threading import Thread
from webbrowser import open as web_open
from flask import render_template, request, jsonify, session, redirect, url_for

# Support both package-relative and direct module imports
try:
    from .system_metrics import cpu_percent, virtual_memory  # type: ignore
except Exception:
    from system_metrics import cpu_percent, virtual_memory  # type: ignore


def register_routes(app, worker):
    """Register all Flask routes on the given app for the provided worker."""

    @app.route('/')
    def index():
        available_locations = ["Asia", "Africa", "North America", "South America", "Europe", "Oceania", "Unknown"]
        return render_template(
            'login.html',
            node_id=worker.node_id,
            current_status=worker.status,
            current_location=worker.location,
            available_locations=available_locations,
        )

    @app.route('/monitor')
    def monitor():
        session_id = session.get('session_id')
        user_data = worker._get_user_session(session_id) if session_id else None

        if not user_data:
            return redirect(url_for('index'))

        available_locations = ["Asia", "Africa", "North America", "South America", "Europe", "Oceania", "Unknown"]
        return render_template(
            'monitor.html',
            username=user_data['username'],
            node_id=worker.node_id,
            initial_status=worker.status,
            current_location=worker.location,
            available_locations=available_locations,
        )

    @app.route('/login', methods=['GET', 'POST'])
    def login_route():
        if request.method == 'GET':
            session_id = session.get('session_id')
            user_data = worker._get_user_session(session_id) if session_id else None

            if user_data and user_data['username'] == worker.username:
                return redirect(url_for('monitor'))

            available_locations = ["Asia", "Africa", "North America", "South America", "Europe", "Oceania", "Unknown"]
            return render_template(
                'login.html',
                node_id=worker.node_id,
                current_status=worker.status,
                current_location=worker.location,
                available_locations=available_locations,
            )

        # POST 登入
        username = request.form.get('username')
        password = request.form.get('password')
        selected_location = request.form.get('location')

        # 更新地區設定
        if selected_location:
            success, message = worker.update_location(selected_location)
            if not success:
                available_locations = ["Asia", "Africa", "North America", "South America", "Europe", "Oceania", "Unknown"]
                return render_template(
                    'login.html',
                    error=f"Location setting error: {message}",
                    node_id=worker.node_id,
                    current_status=worker.status,
                    current_location=worker.location,
                    available_locations=available_locations,
                )

        if not username or not password:
            available_locations = ["Asia", "Africa", "North America", "South America", "Europe", "Oceania", "Unknown"]
            return render_template(
                'login.html',
                error="Please enter username and password",
                node_id=worker.node_id,
                current_status=worker.status,
                current_location=worker.location,
                available_locations=available_locations,
            )

        if worker._login(username, password):
            # 登入成功，現在嘗試註冊
            worker._log(f"Login successful for user '{username}', attempting registration...")

            if worker._register():
                session_id = worker._create_user_session(username, worker.token)
                session['session_id'] = session_id
                session.permanent = True

                worker._log(f"User '{username}' logged in and registered successfully, location: {worker.location}")
                return redirect(url_for('monitor'))
            else:
                # 登入成功但註冊失敗
                worker._log(f"Login successful but registration failed for user '{username}'. Status: {worker.status}", logging.ERROR)
                available_locations = ["Asia", "Africa", "North America", "South America", "Europe", "Oceania", "Unknown"]
                return render_template(
                    'login.html',
                    error=f"Registration failed: {worker.status}",
                    node_id=worker.node_id,
                    current_status=worker.status,
                    current_location=worker.location,
                    available_locations=available_locations,
                )
        else:
            # 登入失敗
            worker._log(f"Login failed for user '{username}'. Status: {worker.status}", logging.ERROR)
            available_locations = ["Asia", "Africa", "North America", "South America", "Europe", "Oceania", "Unknown"]
            return render_template(
                'login.html',
                error=f"Login failed: {worker.status}",
                node_id=worker.node_id,
                current_status=worker.status,
                current_location=worker.location,
                available_locations=available_locations,
            )

    @app.route('/api/update_location', methods=['POST'])
    def api_update_location():
        session_id = session.get('session_id')
        user_data = worker._get_user_session(session_id) if session_id else None

        if not user_data:
            return jsonify({'success': False, 'error': 'Unauthorized'}), 401

        try:
            data = request.get_json()
            new_location = data.get('location')

            if not new_location:
                return jsonify({'success': False, 'error': 'Please select a location'})

            success, message = worker.update_location(new_location)
            return jsonify({'success': success, 'message': message, 'current_location': worker.location})

        except Exception as e:
            return jsonify({'success': False, 'error': f'Update failed: {str(e)}'})

    @app.route('/api/status')
    def api_status():
        session_id = session.get('session_id')
        user_data = worker._get_user_session(session_id) if session_id else None

        # 修復：如果沒有有效會話但有登錄用戶，允許訪問
        if not user_data and worker.username:
            # 創建臨時會話數據用於 API 響應
            user_data = {
                'username': worker.username,
                'cpt_balance': worker.cpt_balance,
                'login_time': worker.login_time or datetime.now()
            }

        if not user_data:
            return jsonify({'error': 'Unauthorized'}), 401

        try:
            cpu_percent_val = cpu_percent()
            mem = virtual_memory()
            current_available_gb = round(mem.available / (1024**3), 2)
        except Exception:
            cpu_percent_val, mem = 0, None
            current_available_gb = worker.memory_gb

        # 獲取目前執行中的任務
        with worker.resources_lock:
            task_count = len(worker.running_tasks)
            # 對於前端相容性，如果有任務則使用第一個任務的ID
            current_task_id = next(iter(worker.running_tasks.keys()), None) if task_count > 0 else None

            # 生成任務列表
            tasks = []
            for t_id, task_info in worker.running_tasks.items():
                tasks.append({
                    'id': t_id,
                    'status': task_info.get('status', 'Unknown'),
                    'start_time': datetime.fromtimestamp(task_info.get('start_time', time.time())).isoformat(),
                    'resources': task_info.get('resources', {})
                })

        return jsonify({
            'node_id': worker.node_id,
            'status': worker.status,
            'current_task_id': current_task_id or "None",  # backward compatibility for old frontend
            'is_registered': worker.is_registered,
            'docker_available': worker.docker_available,
            'docker_status': getattr(worker, 'docker_status', 'unknown'),
            'cpu_percent': round(cpu_percent_val, 1),
            'cpu_cores': worker.cpu_cores,
            'memory_percent': round(mem.percent, 1) if mem else 0,
            'memory_used_gb': round(mem.used/(1024**3), 2) if mem else 0,
            'memory_available_gb': current_available_gb,
            'memory_total_gb': getattr(worker, 'total_memory_gb', worker.memory_gb),
            'cpu_score': worker.cpu_score,
            'gpu_score': worker.gpu_score,
            'gpu_name': worker.gpu_name,
            'gpu_memory_gb': worker.gpu_memory_gb,
            'cpt_balance': user_data['cpt_balance'],
            'login_time': user_data['login_time'].isoformat() if isinstance(user_data['login_time'], datetime) else str(user_data['login_time']),
            'ip': getattr(worker, 'local_ip', '127.0.0.1'),
            'task_count': task_count,
            'tasks': tasks,  # add task list
            'available_resources': worker.available_resources,
            'total_resources': worker.total_resources
        })

    @app.route('/api/logs')
    def api_logs():
        session_id = session.get('session_id')
        user_data = worker._get_user_session(session_id) if session_id else None

        # 修復：如果沒有有效會話但有登錄用戶，允許訪問
        if not user_data and worker.username:
            user_data = {'username': worker.username}

        if not user_data:
            return jsonify({'error': 'Unauthorized'}), 401

        with worker.log_lock:
            return jsonify({'logs': list(worker.logs)})

    @app.route('/logout')
    def logout():
        session_id = session.get('session_id')
        if session_id:
            worker._clear_user_session(session_id)

        session.clear()
        worker._logout()
        return redirect(url_for('index'))

    # 添加任務狀態路由
    @app.route('/api/tasks')
    def api_tasks():
        session_id = session.get('session_id')
        user_data = worker._get_user_session(session_id) if session_id else None

        if not user_data and worker.username:
            user_data = {'username': worker.username}

        if not user_data:
            return jsonify({'error': 'Unauthorized'}), 401

        # 返回所有正在運行的任務
        tasks_info = []
        with worker.resources_lock:
            for t_id, task_data in worker.running_tasks.items():
                tasks_info.append({
                    'task_id': t_id,
                    'status': task_data.get('status', 'Unknown'),
                    'start_time': datetime.fromtimestamp(task_data.get('start_time', 0)).isoformat(),
                    'elapsed': round(time.time() - task_data.get('start_time', time.time()), 1),
                    'resources': task_data.get('resources', {})
                })

        return jsonify({
            'tasks': tasks_info,
            'total_resources': worker.total_resources,
            'available_resources': worker.available_resources
        })


def start_flask(app, worker):
    """Start Flask app in a background thread and open browser."""

    def run_flask():
        try:
            app.run(host='0.0.0.0', port=worker.flask_port, debug=False, use_reloader=False, threaded=True)
        except Exception as e:
            worker._log(f"Flask failed to start: {e}", logging.ERROR)
            import os
            os._exit(1)

    # 啟動 Flask 服務
    Thread(target=run_flask, daemon=True).start()
    worker._log(f"Flask started on port {worker.flask_port}")

    # 延遲開啟瀏覽器
    def open_browser():
        time.sleep(2)
        url = f"http://127.0.0.1:{worker.flask_port}"
        try:
            web_open(url)
            worker._log(f"瀏覽器已開啟: {url}")
        except Exception as e:
            worker._log(f"無法開啟瀏覽器: {e}", logging.WARNING)
            worker._log(f"請手動開啟: {url}")

    Thread(target=open_browser, daemon=True).start()
