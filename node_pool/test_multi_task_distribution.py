import unittest
from unittest.mock import MagicMock
from node_manager_service import NodeManagerServiceServicer
from node_manager import NodeManager
import nodepool_pb2

class TestMultiTaskDistribution(unittest.TestCase):

    def setUp(self):
        # Mock Redis client
        redis_client = MagicMock()
        self.node_manager = NodeManager(redis_client=redis_client)
        self.node_manager_service = NodeManagerServiceServicer(node_manager=self.node_manager)

    def test_upload_task_with_sufficient_nodes(self):
        """測試在節點足夠的情況下分發任務"""
        self.node_manager.redis_client.keys.return_value = ["node:1", "node:2", "node:3"]
        self.node_manager.redis_client.hgetall.side_effect = lambda key: {
            "node:1": {"status": "Idle", "available_cpu_score": "10"},
            "node:2": {"status": "Idle", "available_cpu_score": "20"},
            "node:3": {"status": "Idle", "available_cpu_score": "30"},
        }.get(key, {})

        request = nodepool_pb2.UploadTaskRequest(task_id="task1", task_zip=b"data", host_count=2)
        response = self.node_manager_service.UploadTask(request, None)

        self.assertTrue(response.success)
        self.assertEqual(response.message, "任務分發成功")

    def test_upload_task_with_insufficient_nodes(self):
        """測試在節點不足的情況下分發任務"""
        self.node_manager.redis_client.keys.return_value = ["node:1"]
        self.node_manager.redis_client.hgetall.side_effect = lambda key: {
            "node:1": {"status": "Idle", "available_cpu_score": "10"},
        }.get(key, {})

        request = nodepool_pb2.UploadTaskRequest(task_id="task2", task_zip=b"data", host_count=2)
        response = self.node_manager_service.UploadTask(request, None)

        self.assertFalse(response.success)
        self.assertEqual(response.message, "可用節點數不足")

    def test_upload_task_with_host_count_zero(self):
        """測試 host_count 為 0 的情況"""
        self.node_manager.redis_client.keys.return_value = ["node:1", "node:2"]
        self.node_manager.redis_client.hgetall.side_effect = lambda key: {
            "node:1": {"status": "Idle", "available_cpu_score": "10"},
            "node:2": {"status": "Idle", "available_cpu_score": "20"},
        }.get(key, {})

        request = nodepool_pb2.UploadTaskRequest(task_id="task3", task_zip=b"data", host_count=0)
        response = self.node_manager_service.UploadTask(request, None)

        self.assertTrue(response.success)
        self.assertEqual(response.message, "任務分發成功")