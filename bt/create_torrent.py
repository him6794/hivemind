import libtorrent as lt
import sys
import os

def create_private_torrent(file_or_dir_path, tracker_url, output_torrent_path):
    """
    為指定的檔案或資料夾建立一個私有 .torrent 檔案。
    """
    # --- 關鍵修正: 標準化路徑 ---
    # 確保我們使用的是絕對路徑，這可以避免很多問題
    target_path = os.path.abspath(file_or_dir_path)

    if not os.path.exists(target_path):
        print(f"錯誤：路徑 '{target_path}' 不存在。")
        return

    # 1. 建立檔案儲存物件
    fs = lt.file_storage()
    
    # 使用 libtorrent 的 add_files，它能正確處理檔案和資料夾
    # The paths stored in file_storage will be relative to target_path
    lt.add_files(fs, target_path)
    
    if fs.num_files() == 0:
        print(f"錯誤：在 '{target_path}' 中找不到任何檔案可加入 torrent。")
        return
        
    # 2. 建立 torrent
    t = lt.create_torrent(fs, 0)

    # 3. 設定追蹤器 URL
    t.add_tracker(tracker_url, 0)

    # 4. 设定为私有
    t.set_priv(True)
    
    # 5. 設定建立者和註解 (可選)
    t.set_creator('My Torrent Generator v2.0')

    print(f"正在為 '{target_path}' 生成 torrent... 這可能需要一些時間。")
    
    # --- 關鍵修正: 正確設定 piece hashes ---
    # set_piece_hashes 的第二個參數是 content 的根目錄。
    # libtorrent 會將這個路徑與 file_storage 中的相對路徑結合起來，找到磁碟上的檔案。
    # os.path.dirname(target_path) 是包含目標檔案/資料夾的目錄。
    lt.set_piece_hashes(t, os.path.dirname(target_path))
    print("Hash 計算完成。")

    # 6. 生成 torrent 檔案內容
    torrent_data = t.generate()

    # 7. 將內容寫入檔案
    with open(output_torrent_path, "wb") as f:
        f.write(lt.bencode(torrent_data))

    # 8. 驗證並獲取 infohash
    ti = lt.torrent_info(output_torrent_path)
    info_hash = str(ti.info_hash())
    
    print("-" * 40)
    print(f"成功建立私有 torrent 檔案：'{output_torrent_path}'")
    print(f"檔案/資料夾: {target_path}")
    print(f"追蹤器 URL: {tracker_url}")
    print(f"Infohash: {info_hash}")
    print("-" * 40)


if __name__ == '__main__':
    if len(sys.argv) != 4:
        print("使用方法: python create_torrent.py <檔案或資料夾路徑> <追蹤器URL> <輸出的.torrent檔名>")
        print("範例: python create_torrent.py ./my_project http://127.0.0.1:5000/announce my_project.torrent")
        sys.exit(1)
        
    target_path_arg = sys.argv[1]
    tracker_url_arg = sys.argv[2]
    output_path_arg = sys.argv[3]
    
    create_private_torrent(target_path_arg, tracker_url_arg, output_path_arg)