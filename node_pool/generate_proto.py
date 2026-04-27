#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""
生成 protobuf 文件的腳本，使用兼容的版本
"""

import subprocess
import sys
import os

def generate_protobuf():
    """生成 protobuf 文件"""
    proto_path = "D:\\hivemind\\node_pool\\"
    proto_file = "nodepool.proto"
    
    cmd = [
        sys.executable, "-m", "grpc_tools.protoc",
        f"--proto_path={proto_path}",
        "--python_out=.",
        "--grpc_python_out=.",
        f"{proto_path}/{proto_file}"
    ]
    
    print("正在生成 protobuf 文件...")
    print(" ".join(cmd))
    
    try:
        result = subprocess.run(cmd, check=True, capture_output=True, text=True)
        print("生成成功!")
        
        # 檢查生成的文件
        files = ["nodepool_pb2.py", "nodepool_pb2_grpc.py"]
        for file in files:
            if os.path.exists(file):
                print(f"✓ {file} 已生成")
            else:
                print(f"✗ {file} 生成失敗")
                
    except subprocess.CalledProcessError as e:
        print(f"生成失敗: {e}")
        print(f"錯誤輸出: {e.stderr}")

if __name__ == "__main__":
    generate_protobuf()