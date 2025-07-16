import libtorrent as lt
import time
import sys
import os

def private_seeder(torrent_file_path, data_save_path, tracker_url):
    """
    為一個私有 torrent 檔案持續做種。
    """
    if not os.path.exists(torrent_file_path):
        print(f"錯誤: Torrent 檔案 '{torrent_file_path}' 不存在。")
        return
    if not os.path.isdir(data_save_path):
        print(f"錯誤: 數據路徑 '{data_save_path}' 不是一個有效的目錄。")
        return
        
    # 1. 建立 libtorrent session
    settings = {
        'listen_interfaces': '0.0.0.0:6881',
        'user_agent': 'MyFinalSeeder/1.0',
        'alert_mask': lt.alert.category_t.all_categories
    }
    ses = lt.session(settings)

    print("正在啟動做種服務...")
    
    # 2. 載入 .torrent 檔案
    info = lt.torrent_info(torrent_file_path)

    # 3. 將 torrent 加入 session (最穩健的方式)
    # --- 關鍵修正：在 add_torrent 時直接指定所有參數 ---
    params = {
        'ti': info,
        'save_path': data_save_path,
        'storage_mode': lt.storage_mode_t.storage_mode_sparse,
        'trackers': [tracker_url] # libtorrent 2.x 會自動處理字典或字串列表
    }
    h = ses.add_torrent(params)
    print(f"Torrent 已添加，Tracker URL: {tracker_url}")
    # --- 修正結束 ---

    print("正在檢查檔案完整性...")
    h.force_recheck()

    # 等待檔案檢查完成並進入做種狀態
    is_seeding = False
    while not is_seeding:
        s = h.status()
        if s.state == lt.torrent_status.seeding:
            is_seeding = True
        elif s.state == lt.torrent_status.checking_files:
            print(f'\r正在檢查... {s.progress * 100:.2f}%', end='')
        
        alerts = ses.pop_alerts()
        for alert in alerts:
            if isinstance(alert, lt.hash_failed_alert):
                print(f"\n錯誤: 檔案 HASH 檢查失敗。請確認 '{data_save_path}' 中的檔案是否完整。")
                return
            if isinstance(alert, lt.tracker_error_alert):
                print(f"\n[檢查階段追蹤器錯誤] {alert.message()}")

        time.sleep(0.5)

    print(f"\n檔案檢查完成！開始為 '{info.name()}' 做種...")
    print(f"Infohash: {info.info_hash()}")
    print("按 Ctrl+C 停止服務。")

    # 4. 保持腳本運行並顯示狀態
    try:
        while True:
            alerts = ses.pop_alerts()
            for alert in alerts:
                if isinstance(alert, lt.tracker_error_alert):
                    print(f"\n[追蹤器錯誤] {alert.message()}")
                elif isinstance(alert, lt.tracker_reply_alert):
                    print(f"\n[追蹤器回覆] Peers: {alert.num_peers}")

            s = h.status()
            state_str = ['排隊中', '檢查中', '下載元數據', '下載中',
                         '完成', '做種中', '分配空間', '檢查恢復數據'][s.state]

            print(f'\r進度: {s.progress * 100:.2f}% | '
                  f'上傳: {s.upload_rate / 1000:.1f} kB/s | '
                  f'Peers: {s.num_peers} | 狀態: {state_str}', end=' ')

            time.sleep(1)

    except KeyboardInterrupt:
        print("\n服務已停止。")
    finally:
        ses.remove_torrent(h, lt.session.delete_files)
        print("Torrent 已從 session 移除。")

if __name__ == '__main__':
    if len(sys.argv) != 4:
        print("使用方法: python seeder.py <.torrent檔案路徑> <數據檔案儲存路徑> <追蹤器URL>")
        sys.exit(1)

    torrent_path = sys.argv[1]
    save_path = sys.argv[2]
    tracker_url = sys.argv[3]
    
    private_seeder(torrent_path, save_path, tracker_url)