# Hivemind 文檔索引

## 入門與架構

- [使用與部署手冊（繁體中文）](HIVEMIND_MANUAL.zh-TW.md)：架構、DSL、計費、API、建置、設定、部署與故障排查，附原始碼索引。
- [Getting Started](GETTING_STARTED.md)：本機三個前端、Compose smoke、必要設定與瀏覽器測試。
- [平台架構](ARCHITECTURE.md)：控制面、Master／Worker 分工、信任邊界與 enrollment。
- [公開網路限制](PUBLIC_NETWORK_LIMITATIONS.md)：執行、安全、CPT 與服務限制。

## 任務與結算

- [hcode 教學（繁體中文）](hcode-tutorial.zh-TW.md)：第一個程式、JSON input、提交方式、預算與升級步驟。
- [hcode Runtime](HCODE_RUNTIME.md)：`hcode` 語法、計量、限制與整個任務的 CPT 上限。
- [Managed consensus state](managed-consensus-state.md)：副本、quorum、attempt fencing、用量驗證與結算。
- [Operator-pinned ONNX inference](GENERAL_COMPUTE_ONNX.md)：general-compute alpha 的映像、tensor ABI、GPU/provider 綁定與輸出傳輸。

## 建置、介面與測試

- [平台驗證指南](platform-validation-state.md)：單元測試、資料庫、mock、preflight、本機 smoke、外部 VPN、Linux OCI 與 Windows HCS 測試。
- [Reviewed OCI fixture](general-compute-oci-fixture.md)：Linux OCI fixture、policy 與執行要求。
- [shadcn UI 與原生 client](SHADCN_UI.md)：介面樣式、Windows 視窗生命週期、native bridge 與測試方法。
- [Smoke Benchmarks](SMOKE_BENCHMARKS.md)：三個／五個 Worker 的測量方法與 release criteria。
- [貢獻指南](../CONTRIBUTING.md)：Rust／React 開發、測試與提交流程。

## 計畫與歷史分析

以下文件記錄產品目標或歷史分析；當前契約見上方的任務與架構文檔。

- [零設定與跨平台產品目標](PRODUCT_GOAL_ZERO_CONFIG_CROSS_PLATFORM.md)。
- [Managed runtime 演進計畫](MANAGED_RUNTIME_EVOLUTION_PLAN.md)。
- [實用性與效能評估](UTILITY_PERFORMANCE_EVALUATION.md)：2026-06-26 的快照，包含當時的 ZIP 執行、onboarding、計費與外部網路測量。

應用程式位於 `hivemind-rs/`，runtime libraries 位於 `executor-rs/`，Nodepool consensus crate 位於 `hivemind-rs/crates/managed-consensus/`。三個前端分別位於 `frontend/`、`frontend/master-ui/`、`frontend/worker-ui/`。
