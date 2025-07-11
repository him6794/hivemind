import time
import random
import string
import win32gui
import win32con
import win32api
import threading
import sys
import os
import json
from datetime import datetime

# 配置設定
CONFIG_FILE = "vscode_auto_typer_config.json"

def load_config():
    """加載或創建配置文件"""
    default_config = {
        "min_typing_length": 10,
        "max_typing_length": 50,
        "min_delay": 5,
        "max_delay": 30,
        "save_every": 3,  # 每輸入多少次保存一次
        "log_file": "typing_log.txt",
        "active_hours": {"start": 9, "end": 18}  # 工作時間 9:00-18:00
    }
    
    if os.path.exists(CONFIG_FILE):
        try:
            with open(CONFIG_FILE, 'r') as f:
                return json.load(f)
        except:
            return default_config
    else:
        with open(CONFIG_FILE, 'w') as f:
            json.dump(default_config, f, indent=4)
        return default_config

def log_message(message, log_file):
    """記錄日誌信息"""
    timestamp = datetime.now().strftime("%Y-%m-%d %H:%M:%S")
    log_entry = f"[{timestamp}] {message}\n"
    
    with open(log_file, 'a') as f:
        f.write(log_entry)
    
    print(log_entry.strip())

def find_vscode_window():
    """查找 VS Code 窗口句柄"""
    def callback(hwnd, hwnds):
        if win32gui.IsWindowVisible(hwnd):
            window_text = win32gui.GetWindowText(hwnd).lower()
            if "visual studio code" in window_text or "vscode" in window_text:
                hwnds.append(hwnd)
        return True
    
    hwnds = []
    win32gui.EnumWindows(callback, hwnds)
    return hwnds[0] if hwnds else None

def send_keystrokes(hwnd, text):
    """向指定窗口發送按鍵序列（後台操作）"""
    for char in text:
        vk_code = win32api.VkKeyScan(char)
        scan_code = win32api.MapVirtualKey(vk_code & 0xff, 0)
        
        # 發送鍵盤消息
        win32gui.SendMessage(hwnd, win32con.WM_KEYDOWN, vk_code, 0)
        win32gui.SendMessage(hwnd, win32con.WM_CHAR, ord(char), 0)
        win32gui.SendMessage(hwnd, win32con.WM_KEYUP, vk_code, 0)
        
        # 隨機延遲模擬人類輸入
        time.sleep(random.uniform(0.05, 0.15))

def save_file(hwnd):
    """發送 Ctrl+S 保存文件（後台操作）"""
    # 按下 Ctrl
    win32gui.SendMessage(hwnd, win32con.WM_KEYDOWN, win32con.VK_CONTROL, 0)
    # 按下 S
    win32gui.SendMessage(hwnd, win32con.WM_KEYDOWN, ord('S'), 0)
    # 釋放 S
    win32gui.SendMessage(hwnd, win32con.WM_KEYUP, ord('S'), 0)
    # 釋放 Ctrl
    win32gui.SendMessage(hwnd, win32con.WM_KEYUP, win32con.VK_CONTROL, 0)
    time.sleep(0.5)  # 給保存操作一點時間

def generate_code_like_text(min_length, max_length):
    """生成類似程式碼的隨機文字"""
    # 增加程式碼常用的符號和關鍵字
    keywords = ['if', 'else', 'for', 'while', 'def', 'class', 'return', 
                'import', 'from', 'try', 'except', 'with', 'as', 'in']
    symbols = ['(', ')', '{', '}', '[', ']', '=', '==', '!=', '<', '>', 
               '<=', '>=', '+', '-', '*', '/', '%', '//', '**', '&', '|', 
               '^', '~', '<<', '>>', 'and', 'or', 'not', 'is', 'None', 
               'True', 'False', '# Comment', '""" Docstring """', "''' Docstring '''"]
    
    # 混合字母、數字、符號和關鍵字
    text = []
    length = random.randint(min_length, max_length)
    
    while len(text) < length:
        choice = random.random()
        if choice < 0.3:  # 30% 機會使用關鍵字
            text.append(random.choice(keywords))
        elif choice < 0.6:  # 30% 機會使用符號
            text.append(random.choice(symbols))
        else:  # 40% 機會使用隨機字母組合
            word_length = random.randint(1, 8)
            text.append(''.join(random.choices(string.ascii_lowercase, k=word_length)))
        
        # 添加空格或換行
        if random.random() < 0.3:
            text.append('\n')
        else:
            text.append(' ')
    
    return ''.join(text)

def is_working_hours(config):
    """檢查當前是否在工作時間內"""
    now = datetime.now()
    start_hour = config['active_hours']['start']
    end_hour = config['active_hours']['end']
    return start_hour <= now.hour < end_hour

def typing_worker(config):
    """後台輸入工作線程"""
    log_file = config['log_file']
    
    while True:
        try:
            if not is_working_hours(config):
                time.sleep(60)  # 非工作時間每分鐘檢查一次
                continue
            
            vscode_hwnd = find_vscode_window()
            
            if not vscode_hwnd:
                log_message("VS Code 窗口未找到，等待中...", log_file)
                time.sleep(10)
                continue
            
            # 生成隨機文字
            random_text = generate_code_like_text(
                config['min_typing_length'], 
                config['max_typing_length']
            )
            
            # 發送到 VS Code
            send_keystrokes(vscode_hwnd, random_text)
            
            # 換行
            win32gui.SendMessage(vscode_hwnd, win32con.WM_KEYDOWN, win32con.VK_RETURN, 0)
            win32gui.SendMessage(vscode_hwnd, win32con.WM_KEYUP, win32con.VK_RETURN, 0)
            
            # 定期保存
            if random.randint(1, config['save_every']) == 1:
                save_file(vscode_hwnd)
                log_message(f"已輸入並保存: {random_text[:50]}...", log_file)
            else:
                log_message(f"已輸入: {random_text[:50]}...", log_file)
            
            # 隨機延遲
            delay = random.randint(config['min_delay'], config['max_delay'])
            time.sleep(delay)
            
        except Exception as e:
            log_message(f"錯誤發生: {str(e)}", log_file)
            time.sleep(10)

def main():
    config = load_config()
    log_file = config['log_file']
    
    log_message("VS Code 後台自動輸入程式啟動", log_file)
    log_message(f"配置設定: {json.dumps(config, indent=4)}", log_file)
    
    # 啟動工作線程
    worker_thread = threading.Thread(target=typing_worker, args=(config,), daemon=True)
    worker_thread.start()
    
    # 主線程保持運行
    try:
        while True:
            time.sleep(1)
    except KeyboardInterrupt:
        log_message("程式被用戶中斷", log_file)
        sys.exit(0)

if __name__ == "__main__":
    main()