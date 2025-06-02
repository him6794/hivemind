import grpc
from typing import Optional
import nodepool_pb2
import nodepool_pb2_grpc
import logging
import time

# 配置日誌
logging.basicConfig(level=logging.INFO, format='%(asctime)s - %(levelname)s - %(message)s')

class TestConfig:
    """測試配置常量"""
    TEST_USER = "test"
    TEST_PASSWORD = "password"
    NEW_PASSWORD = "password"  # 與之前更新密碼一致
    RECEIVER_USER = "receiver_testuser"
    RECEIVER_PASSWORD = "password"
    NODE_ID = "worker-node-002"
    HOSTNAME = "worker-node-host"
    CPU_CORES = 4
    MEMORY_GB = 8
    TASK_DESCRIPTION = "測試任務"
    CPT_BUDGET = 50
    CHANNEL_ADDRESS = "localhost:50051"
    TIMEOUT = 10  # gRPC 調用超時（秒）

class RPCErrorHandler:
    """gRPC 錯誤處理裝飾器"""
    @staticmethod
    def handle_rpc_error(func):
        def wrapper(*args, **kwargs):
            try:
                return func(*args, **kwargs)
            except grpc.RpcError as e:
                logging.error(f"{func.__name__} Failed: {e.details() or e.code()}")
                print(f"{func.__name__} Failed: {e.details() or e.code()}")
                return None
            except Exception as e:
                logging.error(f"{func.__name__} Unexpected Error: {e}")
                print(f"{func.__name__} Unexpected Error: {e}")
                return None
        return wrapper

class UserServiceTester:
    """用戶服務測試套件"""
    def __init__(self, stub: nodepool_pb2_grpc.UserServiceStub):
        self.stub = stub
        self.current_token = ""

    @RPCErrorHandler.handle_rpc_error
    def test_register(self, username: str, password: str) -> Optional[nodepool_pb2.StatusResponse]:
        """測試用戶註冊"""
        logging.info(f"測試用戶註冊: {username}")
        request = nodepool_pb2.RegisterRequest(username=username, password=password)
        response = self.stub.Register(request, timeout=TestConfig.TIMEOUT)
        print("\n[用戶註冊測試]")
        self._print_response(response)
        return response

    @RPCErrorHandler.handle_rpc_error
    def test_login(self, username: str, password: str) -> Optional[nodepool_pb2.LoginResponse]:
        """測試用戶登錄"""
        logging.info(f"測試用戶登錄: {username}")
        request = nodepool_pb2.LoginRequest(username=username, password=password)
        response = self.stub.Login(request, timeout=TestConfig.TIMEOUT)
        print("\n[用戶登錄測試]")
        self._print_response(response)
        if response and response.success:
            self.current_token = response.token
            print(f"  Token: {response.token}")
        return response

    @RPCErrorHandler.handle_rpc_error
    def test_get_balance(self) -> Optional[nodepool_pb2.GetBalanceResponse]:
        """測試獲取餘額"""
        logging.info("測試獲取餘額")
        request = nodepool_pb2.GetBalanceRequest(token=self.current_token)
        response = self.stub.GetBalance(request, timeout=TestConfig.TIMEOUT)
        print("\n[餘額查詢測試]")
        self._print_response(response)
        if response and response.success:
            print(f"  Balance: {response.balance}")
        return response

    @RPCErrorHandler.handle_rpc_error
    def test_transfer(self, receiver: str, amount: int) -> Optional[nodepool_pb2.StatusResponse]:
        """測試轉賬功能"""
        logging.info(f"測試轉賬: 接收者 {receiver}, 金額 {amount}")
        # 確保接收者存在
        receiver_reg_response = self.test_register(TestConfig.RECEIVER_USER, TestConfig.RECEIVER_PASSWORD)
        if receiver_reg_response and not receiver_reg_response.success and "已存在" not in receiver_reg_response.message:
            logging.warning(f"接收者 {receiver} 註冊失敗且原因異常: {receiver_reg_response.message}")
            return None

        request = nodepool_pb2.TransferRequest(
            token=self.current_token,
            receiver_username=receiver,
            amount=amount
        )
        response = self.stub.Transfer(request, timeout=TestConfig.TIMEOUT)
        print("\n[轉賬功能測試]")
        self._print_response(response)
        return response

    @RPCErrorHandler.handle_rpc_error
    def test_update_password(self, new_password: str) -> Optional[nodepool_pb2.StatusResponse]:
        """測試密碼更新"""
        logging.info("測試密碼更新")
        request = nodepool_pb2.UpdatePasswordRequest(
            token=self.current_token,
            new_password=new_password
        )
        response = self.stub.UpdatePassword(request, timeout=TestConfig.TIMEOUT)
        print("\n[密碼更新測試]")
        self._print_response(response)
        return response

    @staticmethod
    def _print_response(response):
        """通用響應輸出方法"""
        if response:
            print(f"  Success: {response.success}")
            print(f"  Message: {response.message}")

class NodeServiceTester:
    """節點服務測試套件"""
    def __init__(self, stub: nodepool_pb2_grpc.NodeManagerServiceStub):
        self.stub = stub

    @RPCErrorHandler.handle_rpc_error
    def test_register_node(self) -> Optional[nodepool_pb2.StatusResponse]:
        """測試節點註冊"""
        logging.info(f"測試節點註冊: {TestConfig.NODE_ID}")
        request = nodepool_pb2.RegisterWorkerNodeRequest(
            node_id=TestConfig.NODE_ID,
            hostname=TestConfig.HOSTNAME,
            cpu_cores=TestConfig.CPU_CORES,
            memory_gb=TestConfig.MEMORY_GB
        )
        response = self.stub.RegisterWorkerNode(request, timeout=TestConfig.TIMEOUT)
        print("\n[節點註冊測試]")
        self._print_response(response)
        return response

    @RPCErrorHandler.handle_rpc_error
    def test_health_check(self) -> Optional[nodepool_pb2.HealthCheckResponse]:
        """測試健康檢查"""
        logging.info("測試健康檢查")
        request = nodepool_pb2.HealthCheckRequest()
        response = self.stub.HealthCheck(request, timeout=TestConfig.TIMEOUT)
        print("\n[健康檢查測試]")
        if response:
            print(f"  Healthy: {response.healthy}")
            print(f"  Message: {response.message}")
        return response

    @RPCErrorHandler.handle_rpc_error
    def test_report_status(self, status: str = "空闲状态") -> Optional[nodepool_pb2.StatusResponse]:
        """測試狀態報告"""
        logging.info(f"測試狀態報告: {TestConfig.NODE_ID}, 狀態: {status}")
        request = nodepool_pb2.ReportStatusRequest(
            node_id=TestConfig.NODE_ID,
            status_message=status
        )
        response = self.stub.ReportStatus(request, timeout=TestConfig.TIMEOUT)
        print("\n[狀態報告測試]")
        self._print_response(response)
        return response

    @RPCErrorHandler.handle_rpc_error
    def test_get_node_list(self) -> Optional[nodepool_pb2.GetNodeListResponse]:
        """測試獲取節點列表"""
        logging.info("測試獲取節點列表")
        request = nodepool_pb2.GetNodeListRequest()
        response = self.stub.GetNodeList(request, timeout=TestConfig.TIMEOUT)
        print("\n[節點列表查詢測試]")
        if response and response.success:
            print(f"  Success: {response.success}")
            print(f"  Message: {response.message}")
            print("  Online Nodes:")
            for node in response.nodes:
                print(f"    - {node.node_id} ({node.hostname})")
                print(f"      CPU: {node.cpu_cores} cores")
                print(f"      Memory: {node.memory_gb}GB")
                print(f"      Status: {node.status}")
                print(f"      Last Heartbeat: {node.last_heartbeat}")
        return response

    @staticmethod
    def _print_response(response):
        """通用響應輸出方法"""
        if response:
            print(f"  Success: {response.success}")
            print(f"  Message: {response.message}")

class ResourceServiceTester:
    """資源服務測試套件"""
    def __init__(self, stub: nodepool_pb2_grpc.ResourceAllocatorServiceStub):
        self.stub = stub

    @RPCErrorHandler.handle_rpc_error
    def test_request_resources(self, token: str) -> Optional[nodepool_pb2.ResourceResponse]:
        """測試資源請求"""
        logging.info("測試資源請求")
        request = nodepool_pb2.ResourceRequest(
            user_token=token,
            task_description=TestConfig.TASK_DESCRIPTION,
            cpt_budget=TestConfig.CPT_BUDGET
        )
        response = self.stub.RequestResources(request, timeout=TestConfig.TIMEOUT)
        print("\n[資源請求測試]")
        self._print_response(response)
        if response and response.success:
            print(f"  分配節點: {response.worker_node_ids}")
        return response

    @staticmethod
    def _print_response(response):
        """通用響應輸出方法"""
        if response:
            print(f"  Success: {response.success}")
            print(f"  Message: {response.message}")

class WorkerNodeTester:
    """工作節點測試套件"""
    def __init__(self, stub: nodepool_pb2_grpc.WorkerNodeServiceStub):
        self.stub = stub

    @RPCErrorHandler.handle_rpc_error
    def test_execute_task(self, node_id: str) -> Optional[nodepool_pb2.ExecuteTaskResponse]:
        """測試任務執行"""
        logging.info(f"測試任務執行: {node_id}")
        request = nodepool_pb2.ExecuteTaskRequest(
            node_id=node_id,
            task_id="task-001",
            task_description="模擬運算任務"
        )
        response = self.stub.ExecuteTask(request, timeout=TestConfig.TIMEOUT)
        print("\n[任務執行測試]")
        self._print_response(response)
        if response and response.success:
            print(f"  執行結果: {response.result}")
        return response

    @staticmethod
    def _print_response(response):
        """通用響應輸出方法"""
        if response:
            print(f"  Success: {response.success}")
            print(f"  Message: {response.message}")

def run_full_test():
    logging.info("開始完整測試流程")
    with grpc.insecure_channel(TestConfig.CHANNEL_ADDRESS) as channel:
        user_tester = UserServiceTester(nodepool_pb2_grpc.UserServiceStub(channel))
        node_tester = NodeServiceTester(nodepool_pb2_grpc.NodeManagerServiceStub(channel))
        resource_tester = ResourceServiceTester(nodepool_pb2_grpc.ResourceAllocatorServiceStub(channel))
        worker_tester = WorkerNodeTester(nodepool_pb2_grpc.WorkerNodeServiceStub(channel))

        # 用戶服務測試
        user_tester.test_register(TestConfig.TEST_USER, TestConfig.TEST_PASSWORD)
        login_response = user_tester.test_login(TestConfig.TEST_USER, TestConfig.NEW_PASSWORD)
        
        if login_response and login_response.success:
            user_tester.test_get_balance()
            user_tester.test_transfer(TestConfig.RECEIVER_USER, 10)
            user_tester.test_update_password(TestConfig.NEW_PASSWORD)

        # 節點服務測試
        node_reg_response = node_tester.test_register_node()
        if node_reg_response and node_reg_response.success:
            node_tester.test_report_status("運行中狀態")  # 先報告狀態
            node_tester.test_get_node_list()  # 再獲取列表
        node_tester.test_health_check()

        # 資源服務測試
        if login_response and login_response.success:
            resource_tester.test_request_resources(user_tester.current_token)

        # 工作節點測試
        worker_tester.test_execute_task(TestConfig.NODE_ID)
    
    logging.info("完整測試流程結束")
    
if __name__ == '__main__':
    run_full_test()