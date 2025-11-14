#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""
Node Pool 監控服務
提供 Worker 節點狀態監控的 API 和 Web 介面
"""

import json
import time
import logging
from datetime import datetime, timedelta
from flask import Flask, jsonify, render_template, request
from flask_cors import CORS
import redis
from node_manager import NodeManager
from config import Config

logging.basicConfig(level=logging.INFO, format='%(asctime)s - %(levelname)s - %(message)s')

class NodeMonitorService:
    def __init__(self, port=5001):
        self.app = Flask(__name__, 
                        template_folder='templates',
                        static_folder='static')
        self.app.config['JSON_AS_ASCII'] = False
        CORS(self.app)  # 允許跨域請求
        
        self.port = port
        self.node_manager = NodeManager()
        self.redis_client = redis.Redis(host='localhost', port=6379, db=0, decode_responses=True)
        
        self._setup_routes()

    def _get_all_nodes_status(self):
        """從 Redis 直接取得所有節點狀態（不過濾離線），用於摘要/健康/性能統計"""
        try:
            nodes = []
            now = time.time()
            threshold = getattr(Config, 'HEARTBEAT_ONLINE_THRESHOLD_SECONDS', 180)
            keys = self.redis_client.keys("node:*")
            for key in keys:
                info = self.redis_client.hgetall(key)
                node_id = key.split(":", 1)[1]
                # 轉型並提供預設值
                def to_int(v, default=0):
                    try:
                        return int(v)
                    except Exception:
                        return default
                def to_float(v, default=0.0):
                    try:
                        return float(v)
                    except Exception:
                        return default

                last_hb = to_float(info.get('last_heartbeat', 0))
                node = {
                    'node_id': node_id,
                    'status': info.get('status', 'Unknown'),
                    'trust_level': info.get('trust_level', 'unknown'),
                    'docker_status': info.get('docker_status', 'unknown'),
                    'current_tasks': to_int(info.get('current_tasks', 0)),
                    'last_heartbeat': last_hb,
                    # 即時使用（若 worker 有回報 TaskUsage/ReportStatus）
                    'current_cpu_usage': to_int(info.get('current_cpu_usage', 0)),
                    'current_memory_usage': to_int(info.get('current_memory_usage', 0)),
                    'current_gpu_usage': to_int(info.get('current_gpu_usage', 0)),
                    'current_gpu_memory_usage': to_int(info.get('current_gpu_memory_usage', 0)),
                    # 資源欄位
                    'total_cpu_score': to_int(info.get('total_cpu_score', info.get('cpu_score', 0))),
                    'total_memory_gb': to_float(info.get('total_memory_gb', info.get('memory_gb', 0))),
                    'total_gpu_score': to_int(info.get('total_gpu_score', info.get('gpu_score', 0))),
                    'available_cpu_score': to_int(info.get('available_cpu_score', info.get('cpu_score', 0))),
                    'available_memory_gb': to_float(info.get('available_memory_gb', info.get('memory_gb', 0))),
                    'available_gpu_score': to_int(info.get('available_gpu_score', info.get('gpu_score', 0))),
                }
                # 夾限: available 不可小於 0 或大於 total，避免前端出現負百分比
                node['available_cpu_score'] = max(0, min(node['available_cpu_score'], node['total_cpu_score']))
                node['available_memory_gb'] = max(0.0, min(node['available_memory_gb'], node['total_memory_gb']))
                node['available_gpu_score'] = max(0, min(node['available_gpu_score'], node['total_gpu_score']))
                # 推算是否在線
                node['is_online'] = (now - last_hb) < threshold
                nodes.append(node)
            return nodes
        except Exception as e:
            logging.error(f"讀取所有節點狀態失敗: {e}", exc_info=True)
            return []

    def _enhance_node_info(self, node_info: dict) -> dict:
        """補充節點資訊（在線狀態、時間格式等）"""
        try:
            last_hb = float(node_info.get('last_heartbeat', 0))
            now = time.time()
            enhanced = dict(node_info)
            enhanced.update({
                'is_online': (now - last_hb) < getattr(Config, 'HEARTBEAT_ONLINE_THRESHOLD_SECONDS', 180),
                'last_seen': datetime.fromtimestamp(last_hb).isoformat() if last_hb else None,
                'offline_duration': max(0, now - last_hb) if last_hb else None
            })
            return enhanced
        except Exception:
            return node_info
    
    def _setup_routes(self):
        """設置所有路由"""
        
        @self.app.route('/')
        def dashboard():
            """主監控面板"""
            return render_template('monitor_dashboard.html')
        
        @self.app.route('/api/nodes')
        def api_get_all_nodes():
            """獲取所有節點狀態的 API"""
            try:
                # 先執行清理，移除長時間離線的節點
                self.node_manager.cleanup_offline_nodes(offline_threshold=300)  # 5分鐘
                
                # 直接使用 NodeManager 的 get_node_list 方法
                nodes = self.node_manager.get_node_list()
                
                # 使用 NodeManager 的集群健康狀態方法
                health_status = self.node_manager.get_cluster_health_status()
                
                if health_status:
                    stats = health_status
                else:
                    stats = self._calculate_cluster_stats(nodes)
                
                return jsonify({
                    'success': True,
                    'timestamp': datetime.now().isoformat(),
                    'total_nodes': len(nodes),
                    'stats': stats,
                    'nodes': nodes
                })
            except Exception as e:
                logging.error(f"獲取節點狀態失敗: {e}", exc_info=True)
                return jsonify({
                    'success': False,
                    'error': str(e)
                }), 500
        
        @self.app.route('/api/node/<node_id>')
        def api_get_node_detail(node_id):
            """獲取單個節點詳細狀態"""
            try:
                node_info = self.node_manager.get_node_info(node_id)
                if not node_info:
                    return jsonify({
                        'success': False,
                        'error': f'節點 {node_id} 不存在'
                    }), 404
                
                # 增強節點信息
                enhanced_info = self._enhance_node_info(node_info)
                
                return jsonify({
                    'success': True,
                    'node': enhanced_info
                })
            except Exception as e:
                logging.error(f"獲取節點 {node_id} 詳細信息失敗: {e}", exc_info=True)
                return jsonify({
                    'success': False,
                    'error': str(e)
                }), 500
        
        @self.app.route('/api/nodes/summary')
        def api_nodes_summary():
            """獲取節點摘要統計"""
            try:
                nodes = self._get_all_nodes_status()
                stats = self._calculate_cluster_stats(nodes)
                
                # 按狀態分組
                status_groups = {}
                for node in nodes:
                    status = node.get('status', 'Unknown')
                    if status not in status_groups:
                        status_groups[status] = []
                    status_groups[status].append(node['node_id'])
                
                # 按信任等級分組
                trust_groups = {}
                for node in nodes:
                    trust = node.get('trust_level', 'unknown')
                    if trust not in trust_groups:
                        trust_groups[trust] = 0
                    trust_groups[trust] += 1
                
                return jsonify({
                    'success': True,
                    'summary': {
                        'total_nodes': len(nodes),
                        'online_nodes': stats['online_nodes'],
                        'offline_nodes': stats['offline_nodes'],
                        'busy_nodes': stats['busy_nodes'],
                        'status_distribution': status_groups,
                        'trust_distribution': trust_groups,
                        'total_resources': stats['total_resources'],
                        'available_resources': stats['available_resources'],
                        'resource_utilization': stats['resource_utilization']
                    }
                })
            except Exception as e:
                logging.error(f"獲取節點摘要失敗: {e}", exc_info=True)
                return jsonify({
                    'success': False,
                    'error': str(e)
                }), 500
        
        @self.app.route('/api/nodes/health')
        def api_nodes_health():
            """獲取節點健康狀態"""
            try:
                nodes = self._get_all_nodes_status()
                health_report = []
                
                for node in nodes:
                    last_heartbeat = float(node.get('last_heartbeat', 0))
                    current_time = time.time()
                    offline_threshold = getattr(Config, 'HEARTBEAT_ONLINE_THRESHOLD_SECONDS', 180)
                    
                    health_status = {
                        'node_id': node['node_id'],
                        'is_online': (current_time - last_heartbeat) < offline_threshold,
                        'last_seen': datetime.fromtimestamp(last_heartbeat).isoformat(),
                        'offline_duration': max(0, current_time - last_heartbeat),
                        'status': node.get('status', 'Unknown'),
                        'current_tasks': int(node.get('current_tasks', 0)),
                        'trust_level': node.get('trust_level', 'unknown')
                    }
                    health_report.append(health_status)
                
                # 按離線時間排序
                health_report.sort(key=lambda x: x['offline_duration'], reverse=True)
                
                return jsonify({
                    'success': True,
                    'health_report': health_report,
                    'timestamp': datetime.now().isoformat()
                })
            except Exception as e:
                logging.error(f"獲取節點健康狀態失敗: {e}", exc_info=True)
                return jsonify({
                    'success': False,
                    'error': str(e)
                }), 500

        @self.app.route('/api/cluster/performance')
        def api_cluster_performance():
            """獲取集群性能指標"""
            try:
                nodes = self._get_all_nodes_status()
                
                # 計算集群性能指標
                performance_metrics = {
                    'total_cpu_score': 0,
                    'total_memory_gb': 0,
                    'total_gpu_score': 0,
                    'available_cpu_score': 0,
                    'available_memory_gb': 0,
                    'available_gpu_score': 0,
                    'cpu_utilization_percent': 0,
                    'memory_utilization_percent': 0,
                    'gpu_utilization_percent': 0,
                    'active_tasks': 0,
                    'node_distribution': {
                        'high_trust': 0,
                        'normal_trust': 0,
                        'low_trust': 0,
                        'docker_enabled': 0,
                        'docker_disabled': 0
                    }
                }
                
                for node in nodes:
                    # 累計總資源
                    performance_metrics['total_cpu_score'] += int(node.get('total_cpu_score', 0))
                    performance_metrics['total_memory_gb'] += float(node.get('total_memory_gb', 0))
                    performance_metrics['total_gpu_score'] += int(node.get('total_gpu_score', 0))
                    
                    # 累計可用資源
                    performance_metrics['available_cpu_score'] += int(node.get('available_cpu_score', 0))
                    performance_metrics['available_memory_gb'] += float(node.get('available_memory_gb', 0))
                    performance_metrics['available_gpu_score'] += int(node.get('available_gpu_score', 0))
                    
                    # 累計活動任務
                    performance_metrics['active_tasks'] += int(node.get('current_tasks', 0))
                    
                    # 節點分佈統計
                    trust_level = node.get('trust_level', 'unknown')
                    if trust_level in ['high', 'normal', 'low']:
                        performance_metrics['node_distribution'][f'{trust_level}_trust'] += 1
                    
                    docker_status = node.get('docker_status', 'unknown')
                    if docker_status == 'available':
                        performance_metrics['node_distribution']['docker_enabled'] += 1
                    else:
                        performance_metrics['node_distribution']['docker_disabled'] += 1
                
                # 夾限 available <= total，計算利用率並夾到 [0,100]
                if performance_metrics['total_cpu_score'] > 0:
                    avail = min(max(performance_metrics['available_cpu_score'], 0), performance_metrics['total_cpu_score'])
                    util = (1 - avail / performance_metrics['total_cpu_score']) * 100
                    performance_metrics['cpu_utilization_percent'] = round(max(0, min(100, util)), 2)
                
                if performance_metrics['total_memory_gb'] > 0:
                    avail = min(max(performance_metrics['available_memory_gb'], 0.0), float(performance_metrics['total_memory_gb']))
                    util = (1 - avail / performance_metrics['total_memory_gb']) * 100
                    performance_metrics['memory_utilization_percent'] = round(max(0, min(100, util)), 2)
                
                if performance_metrics['total_gpu_score'] > 0:
                    avail = min(max(performance_metrics['available_gpu_score'], 0), performance_metrics['total_gpu_score'])
                    util = (1 - avail / performance_metrics['total_gpu_score']) * 100
                    performance_metrics['gpu_utilization_percent'] = round(max(0, min(100, util)), 2)
                
                return jsonify({
                    'success': True,
                    'performance': performance_metrics,
                    'timestamp': datetime.now().isoformat()
                })
            except Exception as e:
                logging.error(f"獲取集群性能指標失敗: {e}", exc_info=True)
                return jsonify({
                    'success': False,
                    'error': str(e)
                }), 500

        @self.app.route('/api/cleanup', methods=['POST'])
        def api_cleanup_offline_nodes():
            """手動清理離線節點"""
            try:
                offline_threshold = request.json.get('threshold', 300) if request.is_json else 300
                cleaned_count = self.node_manager.cleanup_offline_nodes(offline_threshold)
                
                return jsonify({
                    'success': True,
                    'cleaned_nodes': cleaned_count,
                    'message': f'已清理 {cleaned_count} 個離線節點',
                    'timestamp': datetime.now().isoformat()
                })
            except Exception as e:
                logging.error(f"清理離線節點失敗: {e}", exc_info=True)
                return jsonify({
                    'success': False,
                    'error': str(e)
                }), 500

        @self.app.route('/api/force_refresh')
        def api_force_refresh():
            """強制刷新所有節點狀態"""
            try:
                # 清理離線節點
                cleaned_count = self.node_manager.cleanup_offline_nodes(offline_threshold=180)  # 3分鐘
                
                # 獲取最新狀態
                nodes = self.node_manager.get_node_list()
                health_status = self.node_manager.get_cluster_health_status()
                
                return jsonify({
                    'success': True,
                    'cleaned_nodes': cleaned_count,
                    'total_nodes': len(nodes),
                    'health_status': health_status,
                    'timestamp': datetime.now().isoformat()
                })
            except Exception as e:
                logging.error(f"強制刷新失敗: {e}", exc_info=True)
                return jsonify({
                    'success': False,
                    'error': str(e)
                }), 500

    def _calculate_cluster_stats(self, nodes):
        """備用集群統計計算方法（如果 NodeManager 方法失敗時使用）"""
        stats = {
            'online_nodes': 0,
            'offline_nodes': 0,
            'busy_nodes': 0,
            'idle_nodes': 0,
            'total_tasks': 0,
            'total_resources': {
                'cpu_score': 0,
                'memory_gb': 0,
                'gpu_score': 0
            },
            'available_resources': {
                'cpu_score': 0,
                'memory_gb': 0,
                'gpu_score': 0
            },
            'resource_utilization': {
                'cpu_percent': 0,
                'memory_percent': 0,
                'gpu_percent': 0
            }
        }
        
        if not nodes:
            return stats
        
        for node in nodes:
            # 統計節點狀態
            if node.get('is_online', False):
                stats['online_nodes'] += 1
                current_tasks = int(node.get('current_tasks', 0))
                if current_tasks > 0:
                    stats['busy_nodes'] += 1
                else:
                    stats['idle_nodes'] += 1
                stats['total_tasks'] += current_tasks
            else:
                stats['offline_nodes'] += 1
            
            # 累計資源
            stats['total_resources']['cpu_score'] += int(node.get('total_cpu_score', 0))
            stats['total_resources']['memory_gb'] += float(node.get('total_memory_gb', 0))
            stats['total_resources']['gpu_score'] += int(node.get('total_gpu_score', 0))
            
            stats['available_resources']['cpu_score'] += int(node.get('available_cpu_score', 0))
            stats['available_resources']['memory_gb'] += float(node.get('available_memory_gb', 0))
            stats['available_resources']['gpu_score'] += int(node.get('available_gpu_score', 0))
        
        # 計算利用率（含夾限，避免 -% 或 >100%）
        total_cpu = stats['total_resources']['cpu_score']
        if total_cpu > 0:
            avail_cpu = min(max(stats['available_resources']['cpu_score'], 0), total_cpu)
            cpu_util = (1 - avail_cpu / total_cpu) * 100
            stats['resource_utilization']['cpu_percent'] = round(max(0, min(100, cpu_util)), 2)
        
        total_memory = stats['total_resources']['memory_gb']
        if total_memory > 0:
            avail_mem = min(max(stats['available_resources']['memory_gb'], 0.0), float(total_memory))
            mem_util = (1 - avail_mem / total_memory) * 100
            stats['resource_utilization']['memory_percent'] = round(max(0, min(100, mem_util)), 2)
        
        total_gpu = stats['total_resources']['gpu_score']
        if total_gpu > 0:
            avail_gpu = min(max(stats['available_resources']['gpu_score'], 0), total_gpu)
            gpu_util = (1 - avail_gpu / total_gpu) * 100
            stats['resource_utilization']['gpu_percent'] = round(max(0, min(100, gpu_util)), 2)
        
        return stats
    
    def run(self, debug=False):
        """啟動監控服務"""
        logging.info(f"啟動 Node Pool 監控服務於端口 {self.port}")
        logging.info(f"訪問地址: http://localhost:{self.port}")
        self.app.run(host='0.0.0.0', port=self.port, debug=debug)

if __name__ == '__main__':
    import argparse
    
    parser = argparse.ArgumentParser(description='Node Pool 監控服務')
    parser.add_argument('--port', type=int, default=5002, help='服務端口 (預設: 5001)')
    parser.add_argument('--debug', action='store_true', help='開啟除錯模式')
    
    args = parser.parse_args()
    
    try:
        monitor = NodeMonitorService(port=args.port)
        monitor.run(debug=args.debug)
    except KeyboardInterrupt:
        logging.info("監控服務已停止")
    except Exception as e:
        logging.error(f"監控服務啟動失敗: {e}", exc_info=True)
