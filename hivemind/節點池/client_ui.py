import sys
import grpc
import nodepool_pb2
import nodepool_pb2_grpc
from PyQt5.QtWidgets import QApplication, QWidget, QVBoxLayout, QPushButton, QLabel, QLineEdit, QComboBox

class ClientUI(QWidget):
    def __init__(self):
        super().__init__()
        self.init_ui()

    def init_ui(self):
        # 設置窗口
        self.setWindowTitle('Node Pool Client')
        self.setGeometry(300, 300, 400, 250)

        # 創建佈局
        layout = QVBoxLayout()

        # 顯示訊息的標籤
        self.response_label = QLabel("Response will be shown here.", self)
        layout.addWidget(self.response_label)

        # 顯示 IP 輸入框
        self.ip_input = QLineEdit(self)
        self.ip_input.setPlaceholderText("Enter Node IP (e.g., 192.168.1.1)")
        layout.addWidget(self.ip_input)

        # 下拉選擇框
        self.request_type = QComboBox(self)
        self.request_type.addItem("Register")
        self.request_type.addItem("Get")
        self.request_type.addItem("Update Status")
        layout.addWidget(self.request_type)

        # 提交按鈕
        self.submit_button = QPushButton('Submit Request', self)
        self.submit_button.clicked.connect(self.on_submit)
        layout.addWidget(self.submit_button)

        self.setLayout(layout)

    def on_submit(self):
        request_type = self.request_type.currentText()
        ip = self.ip_input.text()  # 從輸入框中獲取IP

        if request_type == "Register":
            self.send_register_request(ip)
        elif request_type == "Get":
            self.send_get_request(ip)
        elif request_type == "Update Status":
            self.send_update_status_request(ip)

    def send_register_request(self, ip):
        """發送註冊請求"""
        channel = grpc.insecure_channel('localhost:50051')
        stub = nodepool_pb2_grpc.NodePoolStub(channel)

        node = nodepool_pb2.Node(
            ip=ip,
            cpu_score=5000,
            gpu_score=4000,
            memory=16,
            network_delay=10,
            geographic_location="Taiwan"
        )
        response = stub.Register(node)
        self.response_label.setText(f"Register Response: {response.message}")

    def send_get_request(self, ip):
        """發送 Get 請求"""
        channel = grpc.insecure_channel('localhost:50051')
        stub = nodepool_pb2_grpc.NodePoolStub(channel)

        node_request = nodepool_pb2.NodeRequest(cpu_score=5000, gpu_score=4000)
        response = stub.Get(node_request)
        self.response_label.setText(f"Get Response: ip={response.ip}, cpu_score={response.cpu_score}, "
                                    f"gpu_score={response.gpu_score}, memory={response.memory}, "
                                    f"network_delay={response.network_delay}, "
                                    f"geographic_location={response.geographic_location}")

    def send_update_status_request(self, ip):
        """發送更新狀態請求"""
        channel = grpc.insecure_channel('localhost:50051')
        stub = nodepool_pb2_grpc.NodePoolStub(channel)

        # 確保 ip 作為字符串傳遞
        update_request = nodepool_pb2.UpdateRequest(ip=ip, status="Active")
        response = stub.UpdateStatus(update_request)
        self.response_label.setText(f"Update Status Response: {response.message}")

def main():
    app = QApplication(sys.argv)
    ex = ClientUI()
    ex.show()
    sys.exit(app.exec_())

if __name__ == '__main__':
    main()