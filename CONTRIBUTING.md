# 貢獻指南

感謝您對 HiveMind 分布式運算平台的關注！我們非常歡迎社群的貢獻，無論是代碼、文檔、測試還是問題報告。

## 參與方式

### 提出想法和建議
- 在 [GitHub Discussions](https://github.com/him6794/hivemind/discussions) 中分享您的想法
- 提交功能請求和改進建議
- 參與技術討論和架構設計

### 報告問題
- 使用 [GitHub Issues](https://github.com/him6794/hivemind/issues) 報告 bug
- 提供詳細的重現步驟和環境信息
- 協助驗證和確認其他用戶報告的問題

### 改進文檔
- 修正文檔中的錯誤或不清楚的地方
- 添加使用案例和教程
- 翻譯文檔到其他語言

### 代碼貢獻
- 修復 bug 和改進功能
- 實現新功能
- 優化性能和安全性
- 編寫和改進測試

## 快速開始

### 1. 設置開發環境
```bash
# 克隆倉庫
git clone https://github.com/him6794/hivemind.git
cd hivemind

# 安裝依賴
pip install -r requirements.txt

# 設置 pre-commit 鉤子
pip install pre-commit
pre-commit install
```

### 2. 代碼檢查

**測試狀態提醒**: 目前專案尚未建立測試框架，這是待開發的重要功能。

```bash
# 代碼格式檢查
flake8 .

# 類型檢查
mypy .

# 自動格式化
black .
isort .
```

## 開發工作流程

### 1. 準備階段
- [ ] Fork 本倉庫到您的 GitHub 帳戶
- [ ] 克隆 Fork 的倉庫到本地
- [ ] 設置開發環境和依賴
- [ ] 驗證環境配置（測試框架待建立）

### 2. 開發階段
- [ ] 創建功能分支：`git checkout -b feature/feature-name`
- [ ] 實現功能或修復問題
- [ ] 編寫相關測試（測試框架待建立）
- [ ] 驗證功能正常運作（測試框架待建立）
- [ ] 更新相關文檔

### 3. 代碼檢查
- [ ] 運行代碼格式檢查：`flake8 .`
- [ ] 運行類型檢查：`mypy .`
- [ ] 自動格式化代碼：`black . && isort .`
- [ ] 測試覆蓋率檢查（待建立測試框架）

---

感謝您對 HiveMind 項目的興趣和貢獻！
