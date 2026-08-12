# Task State: Managed Runtime 演進

## Goal

完整實作 [`MANAGED_RUNTIME_EVOLUTION_PLAN.md`](MANAGED_RUNTIME_EVOLUTION_PLAN.md) 的 M0–M5：保留 `managed-function-v0` 的 deterministic、bounded、proof-friendly 契約，同時交付隔離的 `general-compute-v1` runtime、科學運算 ABI／backend、Worker 與 Nodepool 接線、GPU beta，以及可用性發布 gates。

## Success criteria

- 每個可獨立驗收的小單元都先有能正確失敗的測試，再做最小實作、相容性驗證與本地 Conventional Commit；不 push。
- M0 凍結 v1 request/result、artifact manifest、capability matrix 與 threat model，schema/property tests 全綠且不破壞 v0 proof vectors。
- M1 交付 reference interpreter、bounded supervisor、Minsky／recursion／heap／cancel fixtures，以及 timeout/cancel 的 kill/reap 與 hostile escape gates。
- M2 交付 tensor ABI、dtype/complex、broadcast/reduce、BLAS/LAPACK、FFT、ODE、RNG、Monte Carlo、sparse，並以 NumPy/SciPy/reference golden 驗證數值與 failure semantics。
- M3 完成 Worker runtime routing、CAS/chunk transfer、quota/telemetry、retry/idempotency；Nodepool 只結算可信驗證後的 claims，多節點 E2E 通過。
- M4 完成 CUDA/ROCm capability negotiation、driver/image matrix、device artifacts 與明確 CPU fallback，錯配不誤派。
- M5 完成文件、SDK 範例、benchmark dashboard、support matrix、rollback，且 reproducibility/security/performance/release image digest 全部簽核。
- 每個 milestone 都保存測試命令與結果、fixture/hash、benchmark 原始資料、已知限制、rollback 與 owner；最終逐要求完成 completion audit。

## Status

running

## Current step

M0 契約凍結已完成；下一步進入 M1，先為 bounded process supervisor 的 framed stdin/stdout protocol、timeout/cancel 與 kill/reap 建立 RED tests。M0 的 capability matrix 仍是 supervisor 啟動前的 fail-closed gate。

## Completed

- `be39bb7 refactor(runtime): remove unused Monty executable contract`
  - 移除 Hivemind build、Docker、config 與 Windows worker package 的舊 Monty executable contract。
  - executor workspace 29 tests passed。
  - `hivemind-config` 與 `hivemind-worker-executor` focused `cargo check --locked` passed。
  - Docker Compose release contract 與 Windows worker package contract passed。
- 完成演進計畫文件，明確區分 v0 proof-friendly DSL 與 v1 general compute，並定義 M0–M5 gates；文件尚待本輪狀態修正後獨立提交。
- `f34b8eb feat(runtime): add general compute v1 contracts`
  - 新增獨立 `general-compute-runtime` crate，不依賴 Hivemind DB／scheduler。
  - 凍結 `GeneralComputeRequest`／`GeneralComputeResult`、execution/determinism policy、usage claim、artifact/chunk manifest 與 inline SHA-256 validation。
-  - schema tests 3 passed；executor workspace（v0 + v1）與 Hivemind config/worker consumer checks passed。
- capability/threat validation 小單元（待本次 commit）
  - 新增 typed validation errors、有限 execution quota、read-only filesystem gate、backend/image/worker capability matrix，以及 network/GPU/thread mismatch fail-closed checks。
  - artifact chunk manifest 必須連續且完整覆蓋 bytes；gap、overlap、overflow、checksum mismatch 都拒絕。
  - capability/schema tests 7 passed；executor workspace（含 v0 regression）7+1+3+25 passed；format、`hivemind-config` 與 `hivemind-worker-executor` checks passed。

## Active owners

- Origin：使用者，擁有完整 M0–M5 目標與「每小單元測試、相容性驗證、local commit」驗收規則。
- Coordinator／implementation：Codex。
- Nodepool trust review：M3 接線時需依 `AGENT.md` 的 trusted-authority model 驗收。

## Blockers

- 無。Linux sandbox、scientific image、GPU host 與 verifiability backend 是後續 milestone 的必要工作，不是目前 M0 的外部阻塞。

## Next action

在 `general-compute-runtime` 內建立 capability matrix 與 threat-model validation：要求 backend/image/capability 明確註冊，拒絕 GPU mismatch、network/filesystem policy 違反、無限 quota 與 artifact chunk gap/overlap。

## Next checkpoint

M1 第一個 supervisor protocol 小單元完成 RED → GREEN、M0 schema/capability regression 與 v0 consumer checks 全綠並建立本地 commit；然後更新本檔至 cancellation/kill/reap fixture。

## Notes

- 此檔先前的 `complete` 只代表「舊 Monty 清理與計畫文件」完成，並不代表使用者要求的完整演進計畫完成；2026-08-12 已依實際 scope 修正為 `running`。
- 不要對工作樹中的其他 dirty frontend/API/Monty 核心變更使用 `reset`、`checkout` 或整批刪除；它們不屬於目前小單元。
- `managed-function-v0` 的有限配額與 proof settlement 是 load-bearing 契約，不得為了 v1 任意運算而放寬。
- `general-compute-v1` 必須使用獨立 runtime/version/cost/verifiability contract，不能冒充現有 RISC Zero proof path。
