from flask import Flask, request, jsonify, send_file, abort
import os
import logging
import platform
from werkzeug.utils import secure_filename
from version_manager import VersionManager

# 設置日誌
logging.basicConfig(level=logging.INFO, format='%(asctime)s - %(levelname)s - %(message)s')

app = Flask(__name__)
app.config['MAX_CONTENT_LENGTH'] = 500 * 1024 * 1024  # 500MB 上傳限制

# 初始化版本管理器
version_manager = VersionManager()

def detect_platform_from_user_agent(user_agent: str) -> str:
    """從User-Agent檢測平台"""
    user_agent = user_agent.lower()
    
    if 'windows' in user_agent or 'win32' in user_agent or 'win64' in user_agent:
        return 'windows'
    elif 'mac' in user_agent or 'darwin' in user_agent:
        return 'macos'
    elif 'linux' in user_agent or 'unix' in user_agent:
        return 'linux'
    else:
        return 'windows'  # 默認為Windows

@app.route('/api/version/info', methods=['GET'])
def get_version_info():
    """獲取版本信息API"""
    try:
        versions_data = version_manager.load_versions()
        return jsonify({
            'success': True,
            'current_version': versions_data.get('current_version'),
            'available_versions': list(versions_data.get('versions', {}).keys()),
            'supported_platforms': versions_data.get('platforms', []),
            'supported_components': versions_data.get('components', []),
            'last_updated': versions_data.get('last_updated')
        })
    except Exception as e:
        logging.error(f"獲取版本信息錯誤: {e}")
        return jsonify({'success': False, 'message': str(e)}), 500

@app.route('/api/debug/scan', methods=['POST'])
def manual_scan():
    """手動掃描文件API（調試用）"""
    try:
        api_key = request.headers.get('X-API-Key')
        if api_key != 'your-secret-api-key':
            return jsonify({'success': False, 'message': '無效的API密鑰'}), 401
        
        version_manager.scan_existing_files()
        return jsonify({'success': True, 'message': '文件掃描完成'})
        
    except Exception as e:
        logging.error(f"手動掃描錯誤: {e}")
        return jsonify({'success': False, 'message': f'掃描失敗: {str(e)}'}), 500

@app.route('/api/debug/files', methods=['GET'])
def list_files():
    """列出文件系統中的實際文件（調試用）"""
    try:
        file_info = {}
        
        for platform in version_manager.platforms:
            platform_dir = os.path.join(version_manager.releases_path, platform)
            file_info[platform] = {}
            
            if os.path.exists(platform_dir):
                # 列出平台目錄下的直接文件
                direct_files = []
                for filename in os.listdir(platform_dir):
                    file_path = os.path.join(platform_dir, filename)
                    if os.path.isfile(file_path):
                        direct_files.append({
                            'name': filename,
                            'size': os.path.getsize(file_path),
                            'path': file_path
                        })
                file_info[platform]['direct_files'] = direct_files
                
                # 列出組件子目錄中的文件
                for component in version_manager.components:
                    component_dir = os.path.join(platform_dir, component)
                    component_files = []
                    if os.path.exists(component_dir):
                        for filename in os.listdir(component_dir):
                            file_path = os.path.join(component_dir, filename)
                            if os.path.isfile(file_path):
                                component_files.append({
                                    'name': filename,
                                    'size': os.path.getsize(file_path),
                                    'path': file_path
                                })
                    file_info[platform][component] = component_files
        
        return jsonify({
            'success': True,
            'releases_path': version_manager.releases_path,
            'files': file_info
        })
        
    except Exception as e:
        logging.error(f"列出文件錯誤: {e}")
        return jsonify({'success': False, 'message': f'列出文件失敗: {str(e)}'}), 500

@app.route('/api/download/<component>', methods=['GET'])
def download_component(component):
    """下載指定組件的最新版本API"""
    try:
        # 驗證組件名稱
        if not version_manager.validate_component(component):
            return jsonify({
                'success': False, 
                'message': f'不支援的組件: {component}. 支援的組件: {version_manager.components}'
            }), 400
        
        # 從請求中獲取平台信息
        requested_platform = request.args.get('platform')
        
        if not requested_platform:
            # 自動檢測平台
            user_agent = request.headers.get('User-Agent', '')
            requested_platform = detect_platform_from_user_agent(user_agent)
        
        # 驗證平台
        if not version_manager.validate_platform(requested_platform):
            return jsonify({
                'success': False, 
                'message': f'不支援的平台: {requested_platform}. 支援的平台: {version_manager.platforms}'
            }), 400
        
        logging.info(f"下載請求: 組件={component}, 平台={requested_platform}")
        
        # 獲取最新版本的組件信息
        component_info = version_manager.get_latest_component(requested_platform, component)
        
        if not component_info:
            # 額外的調試信息
            versions_data = version_manager.load_versions()
            logging.error(f"組件查找失敗。可用版本: {list(versions_data.get('versions', {}).keys())}")
            
            return jsonify({
                'success': False, 
                'message': f'沒有找到 {component} 組件在平台 {requested_platform} 的可用版本。請檢查文件是否已正確上傳。'
            }), 404
        
        file_path = component_info['file_path']
        
        if not os.path.exists(file_path):
            logging.error(f"文件不存在: {file_path}")
            return jsonify({
                'success': False, 
                'message': f'文件不存在: {file_path}'
            }), 404
        
        # 增加下載計數
        versions_data = version_manager.load_versions()
        current_version = versions_data.get('current_version')
        version_manager.increment_download_count(current_version, requested_platform, component)
        
        logging.info(f"下載開始: 組件 {component}, 版本 {current_version}, 平台 {requested_platform}, 文件 {file_path}, IP: {request.remote_addr}")
        
        # 返回文件
        return send_file(
            file_path,
            as_attachment=True,
            download_name=f"hivemind-{component}.zip",
            mimetype='application/zip'
        )
        
    except Exception as e:
        logging.error(f"下載組件錯誤: {e}")
        return jsonify({'success': False, 'message': '下載失敗'}), 500

@app.route('/api/download/<component>/<version>', methods=['GET'])
def download_component_version(component, version):
    """下載指定組件的指定版本API"""
    try:
        # 驗證組件名稱
        if not version_manager.validate_component(component):
            return jsonify({
                'success': False, 
                'message': f'不支援的組件: {component}. 支援的組件: {version_manager.components}'
            }), 400
        
        # 從請求中獲取平台信息
        requested_platform = request.args.get('platform')
        
        if not requested_platform:
            user_agent = request.headers.get('User-Agent', '')
            requested_platform = detect_platform_from_user_agent(user_agent)
        
        if not version_manager.validate_platform(requested_platform):
            return jsonify({
                'success': False, 
                'message': f'不支援的平台: {requested_platform}'
            }), 400
        
        # 獲取指定版本的組件信息
        component_info = version_manager.get_component_info(version, requested_platform, component)
        
        if not component_info:
            return jsonify({
                'success': False, 
                'message': f'版本 {version} 的 {component} 組件在平台 {requested_platform} 不存在'
            }), 404
        
        file_path = component_info['file_path']
        
        if not os.path.exists(file_path):
            return jsonify({
                'success': False, 
                'message': '文件不存在'
            }), 404
        
        # 增加下載計數
        version_manager.increment_download_count(version, requested_platform, component)
        
        logging.info(f"下載指定版本: 組件 {component}, 版本 {version}, 平台 {requested_platform}, IP: {request.remote_addr}")
        
        return send_file(
            file_path,
            as_attachment=True,
            download_name=f"hivemind-{component}.zip",
            mimetype='application/zip'
        )
        
    except Exception as e:
        logging.error(f"下載指定版本組件錯誤: {e}")
        return jsonify({'success': False, 'message': '下載失敗'}), 500

@app.route('/api/check-update', methods=['POST'])
def check_update():
    """檢查更新API"""
    try:
        data = request.get_json()
        if not data:
            return jsonify({'success': False, 'message': '缺少請求數據'}), 400
        
        current_version = data.get('current_version')
        client_platform = data.get('platform')
        component = data.get('component', 'worker')  # 默認檢查worker組件
        
        if not current_version:
            return jsonify({'success': False, 'message': '缺少當前版本信息'}), 400
        
        if not client_platform:
            user_agent = request.headers.get('User-Agent', '')
            client_platform = detect_platform_from_user_agent(user_agent)
        
        if not version_manager.validate_platform(client_platform):
            return jsonify({'success': False, 'message': f'不支援的平台: {client_platform}'}), 400
        
        if not version_manager.validate_component(component):
            return jsonify({'success': False, 'message': f'不支援的組件: {component}'}), 400
        
        # 獲取最新版本
        versions_data = version_manager.load_versions()
        latest_version = versions_data.get('current_version')
        
        if not latest_version:
            return jsonify({'success': False, 'message': '無法獲取最新版本信息'}), 500
        
        # 檢查是否有更新
        has_update = version_manager.check_version_newer(current_version, latest_version)
        
        result = {
            'success': True,
            'has_update': has_update,
            'current_version': current_version,
            'latest_version': latest_version,
            'platform': client_platform,
            'component': component
        }
        
        if has_update:
            # 獲取更新信息
            component_info = version_manager.get_component_info(latest_version, client_platform, component)
            if component_info:
                result.update({
                    'download_url': f'/api/download/{component}/{latest_version}?platform={client_platform}',
                    'file_size': component_info.get('file_size', 0),
                    'description': component_info.get('description', ''),
                    'release_date': component_info.get('upload_time', '')
                })
        
        return jsonify(result)
        
    except Exception as e:
        logging.error(f"檢查更新錯誤: {e}")
        return jsonify({'success': False, 'message': '檢查更新失敗'}), 500

@app.route('/api/upload', methods=['POST'])
def upload_version():
    """上傳新版本API（管理員用）"""
    try:
        # 簡單的API密鑰驗證
        api_key = request.headers.get('X-API-Key')
        if api_key != 'your-secret-api-key':  # 實際使用時應該使用安全的密鑰
            return jsonify({'success': False, 'message': '無效的API密鑰'}), 401
        
        version = request.form.get('version')
        platform = request.form.get('platform')
        component = request.form.get('component')
        description = request.form.get('description', '')
        
        if not version or not platform or not component:
            return jsonify({'success': False, 'message': '缺少版本、平台或組件信息'}), 400
        
        if not version_manager.validate_platform(platform):
            return jsonify({'success': False, 'message': f'不支援的平台: {platform}'}), 400
        
        if not version_manager.validate_component(component):
            return jsonify({'success': False, 'message': f'不支援的組件: {component}'}), 400
        
        if 'file' not in request.files:
            return jsonify({'success': False, 'message': '缺少文件'}), 400
        
        file = request.files['file']
        if file.filename == '':
            return jsonify({'success': False, 'message': '未選擇文件'}), 400
        
        if not file.filename.endswith('.zip'):
            return jsonify({'success': False, 'message': '只接受ZIP文件'}), 400
        
        # 保存上傳的文件
        temp_dir = "d:/hivemind/temp"
        os.makedirs(temp_dir, exist_ok=True)
        temp_file = os.path.join(temp_dir, secure_filename(file.filename))
        file.save(temp_file)
        
        # 添加到版本管理
        success = version_manager.add_version(version, platform, component, temp_file, description)
        
        # 清理臨時文件
        if os.path.exists(temp_file):
            os.remove(temp_file)
        
        if success:
            return jsonify({
                'success': True, 
                'message': f'版本 {version} ({platform}/{component}) 上傳成功'
            })
        else:
            return jsonify({'success': False, 'message': '版本添加失敗'}), 500
        
    except Exception as e:
        logging.error(f"上傳版本錯誤: {e}")
        return jsonify({'success': False, 'message': '上傳失敗'}), 500

@app.route('/api/stats', methods=['GET'])
def get_download_stats():
    """獲取下載統計API"""
    try:
        versions_data = version_manager.load_versions()
        stats = {
            'success': True,
            'versions': {},
            'total_downloads': 0
        }
        
        for version, platforms in versions_data.get('versions', {}).items():
            stats['versions'][version] = {}
            for platform, components in platforms.items():
                stats['versions'][version][platform] = {}
                for component, info in components.items():
                    download_count = info.get('download_count', 0)
                    stats['versions'][version][platform][component] = {
                        'download_count': download_count,
                        'file_size': info.get('file_size', 0),
                        'upload_time': info.get('upload_time', '')
                    }
                    stats['total_downloads'] += download_count
        
        return jsonify(stats)
        
    except Exception as e:
        logging.error(f"獲取統計錯誤: {e}")
        return jsonify({'success': False, 'message': '獲取統計失敗'}), 500

@app.errorhandler(404)
def not_found(error):
    return jsonify({'success': False, 'message': 'API端點不存在'}), 404

@app.errorhandler(500)
def internal_error(error):
    return jsonify({'success': False, 'message': '伺服器內部錯誤'}), 500

if __name__ == '__main__':
    print("HiveMind 下載 API 服務器啟動中...")
    print("可用的API端點:")
    print("  GET  /api/version/info              - 獲取版本信息")
    print("  GET  /api/download/<component>      - 下載指定組件最新版本")
    print("  GET  /api/download/<component>/<版本> - 下載指定組件指定版本")
    print("  POST /api/check-update              - 檢查更新")
    print("  POST /api/upload                    - 上傳新版本（需要API密鑰）")
    print("  GET  /api/stats                     - 獲取下載統計")
    print("  POST /api/debug/scan                - 手動掃描文件（需要API密鑰）")
    print("  GET  /api/debug/files               - 列出實際文件")
    print("")
    print("支援的組件: worker, master")
    print("支援的平台: windows, linux, macos")
    print("下載範例:")
    print("  curl http://localhost:5000/api/download/worker?platform=windows")
    print("  curl http://localhost:5000/api/download/master?platform=linux")
    print("")
    print("調試範例:")
    print("  curl http://localhost:5000/api/debug/files")
    print("  curl -X POST -H 'X-API-Key: your-secret-api-key' http://localhost:5000/api/debug/scan")
    
    app.run(host='0.0.0.0', port=5000, debug=False)
