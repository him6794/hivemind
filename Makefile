.PHONY: help dev build test clean proto docker-build docker-up docker-down install-deps

# 預設目標
help:
	@echo "Hivemind 建置工具"
	@echo ""
	@echo "可用命令："
	@echo "  make dev              - 啟動開發環境（所有服務）"
	@echo "  make build            - 建置所有後端服務"
	@echo "  make test             - 執行所有測試"
	@echo "  make test-coverage    - 執行測試並生成覆蓋率報告"
	@echo "  make clean            - 清理建置產物"
	@echo "  make proto            - 生成 protobuf 程式碼"
	@echo "  make docker-build     - 建置 Docker 映像"
	@echo "  make docker-up        - 啟動 Docker Compose"
	@echo "  make docker-down      - 停止 Docker Compose"
	@echo "  make install-deps     - 安裝所有依賴"
	@echo "  make lint             - 執行程式碼檢查"
	@echo "  make fmt              - 格式化程式碼"

# ============================================
# 開發環境
# ============================================

dev: install-deps
	@echo "啟動開發環境..."
	@docker-compose up -d redis postgres
	@echo "等待資料庫啟動..."
	@sleep 5
	@echo "啟動後端服務..."
	@cd services/nodepool/cmd/server && go run . &
	@cd services/master/cmd/server && go run . &
	@cd services/worker/cmd/server && go run . &
	@echo "啟動前端服務..."
	@cd frontend/master-ui && npm run dev &
	@cd frontend/worker-ui && npm run dev &
	@echo "開發環境已啟動！"
	@echo "Master UI: http://localhost:3000"
	@echo "Worker UI: http://localhost:3001"
	@echo "Master API: http://localhost:8082"

# ============================================
# 建置
# ============================================

build: build-nodepool build-master build-worker build-frontend

build-nodepool:
	@echo "建置 Nodepool..."
	@cd services/nodepool/cmd/server && go build -o ../../../../bin/nodepool

build-master:
	@echo "建置 Master..."
	@cd services/master/cmd/server && go build -o ../../../../bin/master

build-worker:
	@echo "建置 Worker..."
	@cd services/worker/cmd/server && go build -o ../../../../bin/worker

build-frontend:
	@echo "建置前端..."
	@cd frontend/master-ui && npm run build
	@cd frontend/worker-ui && npm run build

# ============================================
# 測試
# ============================================

test: test-nodepool test-master test-worker

test-nodepool:
	@echo "測試 Nodepool..."
	@cd services/nodepool && go test -v ./...

test-master:
	@echo "測試 Master..."
	@cd services/master && go test -v ./...

test-worker:
	@echo "測試 Worker..."
	@cd services/worker && go test -v ./...

test-coverage:
	@echo "生成測試覆蓋率報告..."
	@cd services/nodepool && go test -coverprofile=coverage.out ./... && go tool cover -html=coverage.out -o coverage.html
	@cd services/master && go test -coverprofile=coverage.out ./... && go tool cover -html=coverage.out -o coverage.html
	@cd services/worker && go test -coverprofile=coverage.out ./... && go tool cover -html=coverage.out -o coverage.html
	@echo "覆蓋率報告已生成："
	@echo "  services/nodepool/coverage.html"
	@echo "  services/master/coverage.html"
	@echo "  services/worker/coverage.html"

# ============================================
# 程式碼品質
# ============================================

lint:
	@echo "執行程式碼檢查..."
	@cd services/nodepool && golangci-lint run
	@cd services/master && golangci-lint run
	@cd services/worker && golangci-lint run

fmt:
	@echo "格式化程式碼..."
	@cd services/nodepool && go fmt ./...
	@cd services/master && go fmt ./...
	@cd services/worker && go fmt ./...

# ============================================
# Protobuf
# ============================================

proto:
	@echo "生成 protobuf 程式碼..."
	@protoc --go_out=services/nodepool/pb --go-grpc_out=services/nodepool/pb proto/hivemind.proto
	@protoc --go_out=services/nodepool/pb --go-grpc_out=services/nodepool/pb proto/vpn.proto
	@echo "Protobuf 程式碼已生成"

# ============================================
# Docker
# ============================================

docker-build:
	@echo "建置 Docker 映像..."
	@docker-compose build

docker-up:
	@echo "啟動 Docker Compose..."
	@docker-compose up -d
	@echo "服務已啟動！"
	@docker-compose ps

docker-down:
	@echo "停止 Docker Compose..."
	@docker-compose down

docker-logs:
	@docker-compose logs -f

docker-clean:
	@echo "清理 Docker 資源..."
	@docker-compose down -v
	@docker system prune -f

# ============================================
# 依賴管理
# ============================================

install-deps: install-go-deps install-frontend-deps

install-go-deps:
	@echo "安裝 Go 依賴..."
	@cd services/nodepool && go mod download && go mod tidy
	@cd services/master && go mod download && go mod tidy
	@cd services/worker && go mod download && go mod tidy

install-frontend-deps:
	@echo "安裝前端依賴..."
	@cd frontend/master-ui && npm install
	@cd frontend/worker-ui && npm install

# ============================================
# 清理
# ============================================

clean:
	@echo "清理建置產物..."
	@rm -rf bin/
	@rm -rf services/nodepool/coverage.out services/nodepool/coverage.html
	@rm -rf services/master/coverage.out services/master/coverage.html
	@rm -rf services/worker/coverage.out services/worker/coverage.html
	@rm -rf frontend/master-ui/dist
	@rm -rf frontend/worker-ui/dist
	@echo "清理完成"

clean-all: clean docker-clean
	@echo "完全清理完成"

# ============================================
# 資料庫管理
# ============================================

db-reset:
	@echo "重置資料庫..."
	@docker-compose down postgres redis
	@docker volume rm hivemind-postgres-data hivemind-redis-data || true
	@docker-compose up -d postgres redis
	@echo "資料庫已重置"

db-backup:
	@echo "備份資料庫..."
	@mkdir -p backups
	@docker exec hivemind-postgres pg_dump -U hivemind hivemind > backups/backup_$$(date +%Y%m%d_%H%M%S).sql
	@echo "備份完成"

# ============================================
# 開發工具
# ============================================

watch-logs:
	@echo "監控日誌..."
	@tail -f nodepool.log

redis-cli:
	@docker exec -it hivemind-redis redis-cli

psql:
	@docker exec -it hivemind-postgres psql -U hivemind -d hivemind
