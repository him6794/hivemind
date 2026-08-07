# Hivemind 完整驗證計畫

## 目標

完成 managed-function-only 平台的後端、前端、release stack 與瀏覽器端驗證；修復驗證中發現的回歸；以 Conventional Commits 分批提交至本機，不 push。

## 狀態

`complete`

## 已完成

1. 修復 managed runtime 的 cooperative cancellation 與 blocking pool 執行。
2. 完成 GNU backend workspace、managed runtime、三個 frontend 與全部 release contract 測試。
3. 修復 release smoke 的 Windows port collision、固定 volume 汙染、動態 UI CORS 與 preview process cleanup。
4. 修正 billing admission gate 上線後過期的 Playwright 預算 fixture。
5. 完成乾淨隔離 stack 的 Playwright 全流程與最終回歸矩陣。
6. 清理本輪 Docker 隔離資源並停止 native validation PostgreSQL。
7. 分批建立本機 Conventional Commits；不 push、不建立 PR。

## 驗證摘要

完整結果與殘留 cleanup 說明見 `docs/platform-validation-state.md`。
