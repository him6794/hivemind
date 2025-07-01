import os
import json
import logging
import hashlib
from typing import Dict, List, Optional, Tuple
from datetime import datetime

class VersionManager:
    def __init__(self, releases_path="d:/hivemind/releases"):
        self.releases_path = releases_path
        self.versions_file = os.path.join(releases_path, "versions.json")
        self.components = ["worker", "master"]  # 支援的組件類型
        self.platforms = ["windows", "linux", "macos"]  # 支援的平台
        self.ensure_directories()
        
    def ensure_directories(self):
        """確保必要的目錄存在"""
        try:
            os.makedirs(self.releases_path, exist_ok=True)
            # 為每個平台和組件創建目錄
            for platform in self.platforms:
                platform_dir = os.path.join(self.releases_path, platform)
                os.makedirs(platform_dir, exist_ok=True)
                for component in self.components:
                    component_dir = os.path.join(platform_dir, component)
                    os.makedirs(component_dir, exist_ok=True)
            logging.info(f"版本管理目錄已準備: {self.releases_path}")
            
            # 檢查並掃描現有文件
            self.scan_existing_files()
            
        except Exception as e:
            logging.error(f"創建版本目錄失敗: {e}")
            raise
    
    def scan_existing_files(self):
        """掃描現有文件並自動添加到版本管理"""
        try:
            logging.info("開始掃描現有文件...")
            versions_data = self.load_versions()
            files_found = 0
            
            # 固定使用 "latest" 作為版本號，不檢查具體版本
            version = "latest"
            
            for platform in self.platforms:
                platform_dir = os.path.join(self.releases_path, platform)
                if not os.path.exists(platform_dir):
                    continue
                    
                # 檢查平台目錄下的直接ZIP文件
                for filename in os.listdir(platform_dir):
                    if filename.endswith('.zip'):
                        file_path = os.path.join(platform_dir, filename)
                        self._process_zip_file(file_path, platform, versions_data, version)
                        files_found += 1
                
                # 檢查組件子目錄
                for component in self.components:
                    component_dir = os.path.join(platform_dir, component)
                    if os.path.exists(component_dir):
                        for filename in os.listdir(component_dir):
                            if filename.endswith('.zip'):
                                file_path = os.path.join(component_dir, filename)
                                self._process_zip_file(file_path, platform, versions_data, version, component)
                                files_found += 1
            
            if files_found > 0:
                self.save_versions(versions_data)
                logging.info(f"掃描完成，發現 {files_found} 個文件")
            else:
                logging.info("沒有發現現有的ZIP文件")
                
        except Exception as e:
            logging.error(f"掃描現有文件失敗: {e}")
    
    def _process_zip_file(self, file_path, platform, versions_data, version="latest", component=None):
        """處理發現的ZIP文件"""
        try:
            filename = os.path.basename(file_path)
            
            # 從文件名推斷組件（如果未指定）
            if not component:
                if 'worker' in filename.lower():
                    component = 'worker'
                elif 'master' in filename.lower():
                    component = 'master'
                else:
                    # 如果無法從文件名推斷，跳過
                    logging.warning(f"無法從文件名推斷組件類型: {filename}")
                    return
            
            # 計算文件信息
            file_size = os.path.getsize(file_path)
            file_hash = self.get_file_hash(file_path)
            
            # 更新版本信息
            if version not in versions_data["versions"]:
                versions_data["versions"][version] = {}
            
            if platform not in versions_data["versions"][version]:
                versions_data["versions"][version][platform] = {}
            
            versions_data["versions"][version][platform][component] = {
                "file_path": file_path,
                "file_size": file_size,
                "file_hash": file_hash,
                "description": f"最新的 {component} 組件",
                "upload_time": datetime.now().isoformat(),
                "download_count": 0
            }
            
            # 總是使用 "latest" 作為當前版本
            versions_data["current_version"] = version
            
            logging.info(f"已添加文件: {filename} -> 版本 {version}, 平台 {platform}, 組件 {component}")
            
        except Exception as e:
            logging.error(f"處理文件 {file_path} 失敗: {e}")

    def get_file_hash(self, file_path: str) -> str:
        """計算文件的SHA256哈希值"""
        try:
            hash_sha256 = hashlib.sha256()
            with open(file_path, "rb") as f:
                for chunk in iter(lambda: f.read(4096), b""):
                    hash_sha256.update(chunk)
            return hash_sha256.hexdigest()
        except Exception as e:
            logging.error(f"計算文件哈希失敗: {e}")
            return ""
    
    def load_versions(self) -> Dict:
        """載入版本信息"""
        try:
            if os.path.exists(self.versions_file):
                with open(self.versions_file, 'r', encoding='utf-8') as f:
                    return json.load(f)
            else:
                # 創建默認版本文件
                default_versions = {
                    "current_version": "latest",
                    "versions": {},
                    "platforms": self.platforms,
                    "components": self.components,
                    "last_updated": datetime.now().isoformat()
                }
                self.save_versions(default_versions)
                return default_versions
        except Exception as e:
            logging.error(f"載入版本信息失敗: {e}")
            return {"current_version": "latest", "versions": {}, "platforms": self.platforms, "components": self.components}
    
    def save_versions(self, versions_data: Dict):
        """保存版本信息"""
        try:
            versions_data["last_updated"] = datetime.now().isoformat()
            with open(self.versions_file, 'w', encoding='utf-8') as f:
                json.dump(versions_data, f, indent=2, ensure_ascii=False)
            logging.info("版本信息已保存")
        except Exception as e:
            logging.error(f"保存版本信息失敗: {e}")
    
    def validate_component(self, component: str) -> bool:
        """驗證組件名稱"""
        return component in self.components
    
    def validate_platform(self, platform: str) -> bool:
        """驗證平台名稱"""
        return platform in self.platforms
    
    def add_version(self, version: str, platform: str, component: str, file_path: str, description: str = "") -> bool:
        """添加新版本的組件"""
        try:
            if not os.path.exists(file_path):
                logging.error(f"文件不存在: {file_path}")
                return False
            
            if not self.validate_platform(platform):
                logging.error(f"不支援的平台: {platform}")
                return False
            
            if not self.validate_component(component):
                logging.error(f"不支援的組件: {component}")
                return False
            
            versions_data = self.load_versions()
            
            # 固定使用 "latest" 版本
            version = "latest"
            
            # 創建版本目錄
            component_dir = os.path.join(self.releases_path, platform, component)
            target_file = os.path.join(component_dir, f"hivemind-{component}.zip")
            
            # 複製文件到版本目錄
            import shutil
            shutil.copy2(file_path, target_file)
            
            # 計算文件信息
            file_size = os.path.getsize(target_file)
            file_hash = self.get_file_hash(target_file)
            
            # 更新版本信息
            if version not in versions_data["versions"]:
                versions_data["versions"][version] = {}
            
            if platform not in versions_data["versions"][version]:
                versions_data["versions"][version][platform] = {}
            
            versions_data["versions"][version][platform][component] = {
                "file_path": target_file,
                "file_size": file_size,
                "file_hash": file_hash,
                "description": description,
                "upload_time": datetime.now().isoformat(),
                "download_count": 0
            }
            
            # 設置當前版本
            versions_data["current_version"] = version
            
            self.save_versions(versions_data)
            logging.info(f"版本 {version} ({platform}/{component}) 添加成功")
            return True
            
        except Exception as e:
            logging.error(f"添加版本失敗: {e}")
            return False
    
    def get_latest_component(self, platform: str, component: str) -> Optional[Dict]:
        """獲取最新版本的組件信息"""
        try:
            versions_data = self.load_versions()
            
            # 直接查找任何可用的組件，不檢查版本
            logging.debug(f"查找組件: 平台={platform}, 組件={component}")
            
            # 遍歷所有版本，找到第一個匹配的組件就返回
            for version in versions_data.get("versions", {}):
                if platform in versions_data["versions"][version]:
                    if component in versions_data["versions"][version][platform]:
                        component_info = versions_data["versions"][version][platform][component]
                        file_path = component_info.get("file_path")
                        
                        # 檢查文件是否真實存在
                        if file_path and os.path.exists(file_path):
                            logging.info(f"找到組件: {component} (平台: {platform}, 文件: {file_path})")
                            return component_info
                        else:
                            logging.warning(f"組件文件不存在: {file_path}")
            
            logging.error(f"未找到 {component} 組件 (平台: {platform})")
            return None
            
        except Exception as e:
            logging.error(f"獲取最新組件失敗: {e}")
            return None
    
    def get_component_info(self, version: str, platform: str, component: str) -> Optional[Dict]:
        """獲取特定版本、平台和組件的信息"""
        try:
            versions_data = self.load_versions()
            
            logging.debug(f"查找組件信息: 版本={version}, 平台={platform}, 組件={component}")
            
            if version in versions_data["versions"]:
                version_data = versions_data["versions"][version]
                
                if platform in version_data:
                    platform_data = version_data[platform]
                    
                    if component in platform_data:
                        component_info = platform_data[component]
                        file_path = component_info.get("file_path")
                        
                        # 檢查文件是否真實存在
                        if file_path and os.path.exists(file_path):
                            logging.debug(f"找到組件文件: {file_path}")
                            return component_info
                        else:
                            logging.warning(f"組件文件不存在: {file_path}")
                            return None
            
            return None
        except Exception as e:
            logging.error(f"獲取組件信息失敗: {e}")
            return None
    
    def get_version_info(self, version: str, platform: str) -> Optional[Dict]:
        """獲取特定版本和平台的所有組件信息（保持向後兼容）"""
        try:
            versions_data = self.load_versions()
            if (version in versions_data["versions"] and 
                platform in versions_data["versions"][version]):
                return versions_data["versions"][version][platform]
            return None
        except Exception as e:
            logging.error(f"獲取版本信息失敗: {e}")
            return None
    
    def get_latest_version(self, platform: str) -> Optional[Dict]:
        """獲取最新版本的所有組件信息（保持向後兼容）"""
        try:
            versions_data = self.load_versions()
            current_version = versions_data.get("current_version")
            if current_version:
                return self.get_version_info(current_version, platform)
            return None
        except Exception as e:
            logging.error(f"獲取最新版本失敗: {e}")
            return None
    
    def list_versions(self) -> List[str]:
        """列出所有可用版本"""
        try:
            versions_data = self.load_versions()
            return list(versions_data.get("versions", {}).keys())
        except Exception as e:
            logging.error(f"列出版本失敗: {e}")
            return []
    
    def increment_download_count(self, version: str, platform: str, component: str):
        """增加下載計數"""
        try:
            versions_data = self.load_versions()
            if (version in versions_data["versions"] and 
                platform in versions_data["versions"][version] and
                component in versions_data["versions"][version][platform]):
                versions_data["versions"][version][platform][component]["download_count"] += 1
                self.save_versions(versions_data)
        except Exception as e:
            logging.error(f"更新下載計數失敗: {e}")
    
    def check_version_newer(self, current_version: str, target_version: str) -> bool:
        """檢查目標版本是否比當前版本新（簡化版本，總是返回True表示有更新）"""
        # 簡化：總是認為有更新可用
        return True
    
    def get_available_components(self, version: str, platform: str) -> List[str]:
        """獲取指定版本和平台的可用組件列表"""
        try:
            versions_data = self.load_versions()
            if (version in versions_data["versions"] and 
                platform in versions_data["versions"][version]):
                return list(versions_data["versions"][version][platform].keys())
            return []
        except Exception as e:
            logging.error(f"獲取可用組件失敗: {e}")
            return []
    
    def is_version_complete(self, version: str, platform: str) -> bool:
        """檢查版本是否包含所有必需的組件"""
        try:
            available_components = self.get_available_components(version, platform)
            return all(component in available_components for component in self.components)
        except Exception as e:
            logging.error(f"檢查版本完整性失敗: {e}")
            return False
