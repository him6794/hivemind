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

M1 framed stdin/stdout protocol、bounded supervisor lifecycle、bounded stdout/stderr capture 與 process-group／descendant cleanup 小單元已完成；下一步進入 reference interpreter 與 Minsky／recursion／heap／cancellation fixtures。M0 capability matrix 仍是 supervisor 啟動前的 fail-closed gate。

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
- `37ae840 feat(runtime): enforce v1 capabilities and threat boundaries`
  - 新增 typed validation errors、有限 execution quota、read-only filesystem gate、backend/image/worker capability matrix，以及 network/GPU/thread mismatch fail-closed checks。
  - artifact chunk manifest 必須連續且完整覆蓋 bytes；gap、overlap、overflow、checksum mismatch 都拒絕。
  - capability/schema tests 7 passed；executor workspace（含 v0 regression）7+1+3+25 passed；format、`hivemind-config` 與 `hivemind-worker-executor` checks passed。
- `f4ac2b9 feat(runtime): add bounded framed protocol`
  - 新增 4-byte big-endian length-prefixed JSON frame encoder/decoder，先驗證 payload cap，再反序列化；decoder 回傳 consumed bytes 以支援連續 frame。
  - truncated header/payload、oversized payload、invalid JSON、encode cap 與 exact-one-frame consumption tests 4 passed；M0 schema tests 7 passed、executor workspace v0 regression、format、`hivemind-config` 與 `hivemind-worker-executor` checks passed。
- `5b342f1 feat(runtime): add bounded supervisor lifecycle`
  - 新增分離的 program/args 啟動、monotonic timeout、cooperative cancellation，以及 timeout/cancel 後 hard kill + wait/reap；空白 program fail-closed。
  - lifecycle tests 4 passed；executor workspace 全測試、format、`hivemind-config` 與 `hivemind-worker-executor` checks passed。
- `0dea304 feat(runtime): bound supervisor output capture`
  - supervisor 以獨立 reader 持續 drain stdout/stderr，僅保留 `output_limit` bytes，並回傳各 stream 的 truncation 標記，避免 pipe back-pressure 與無界記憶體。
  - lifecycle tests 5 passed；executor workspace 全測試、format、`hivemind-config` 與 `hivemind-worker-executor` checks passed。
- M1 process-group／descendant cleanup 小單元（待本次 commit）
  - timeout/cancel 時 Unix 建立獨立 process group 並以 group kill 清理 descendants；Windows 使用 `taskkill /T /F` 的 tree-kill fallback，完成後 wait/reap。
  - hostile descendant marker fixture 與 lifecycle tests 6 passed；executor workspace 全測試、format、`hivemind-config` 與 `hivemind-worker-executor` checks passed。

## Active owners

- Origin：使用者，擁有完整 M0–M5 目標與「每小單元測試、相容性驗證、local commit」驗收規則。
- Coordinator／implementation：Codex。
- Nodepool trust review：M3 接線時需依 `AGENT.md` 的 trusted-authority model 驗收。

## Blockers

- 無。Linux sandbox、scientific image、GPU host 與 verifiability backend 是後續 milestone 的必要工作，不是目前 M0 的外部阻塞。

## Next action

在 `general-compute-runtime` 內建立 reference interpreter 的最小 bounded language core 與 Minsky machine fixtures，先鎖定 deterministic step、heap、recursion 與 cancellation semantics；Windows Job Object 對等保護另列為 supervisor hardening 小單元。

## Next checkpoint

M1 process-group／descendant cleanup 小單元完成 RED → GREEN、protocol/M0 schema/capability regression 與 v0 consumer checks 全綠並建立本地 commit；下一 checkpoint 為 reference interpreter fixture。

## Notes

- 此檔先前的 `complete` 只代表「舊 Monty 清理與計畫文件」完成，並不代表使用者要求的完整演進計畫完成；2026-08-12 已依實際 scope 修正為 `running`。
- 不要對工作樹中的其他 dirty frontend/API/Monty 核心變更使用 `reset`、`checkout` 或整批刪除；它們不屬於目前小單元。
- `managed-function-v0` 的有限配額與 proof settlement 是 load-bearing 契約，不得為了 v1 任意運算而放寬。
- `general-compute-v1` 必須使用獨立 runtime/version/cost/verifiability contract，不能冒充現有 RISC Zero proof path。
