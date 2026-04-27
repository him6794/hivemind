package storage

import (
	"context"
	"database/sql"
	"fmt"
	"time"

	_ "github.com/jackc/pgx/v5/stdlib"
)

type TaskRecord struct {
	TaskID           string
	Owner            string
	WorkerID         string
	WorkerIP         string
	Status           string
	StatusMessage    string
	Output           string
	ResultTorrent    string
	TorrentSource    string
	ExpectedBTIH     string
	CpuUsage         float32
	MemoryUsage      float32
	GpuUsage         float32
	GpuMemoryUsage   float32
	ReservedCPT      int64
	ReservedRemain   int64
	ReqCPUScore      int32
	ReqGPUScore      int32
	ReqMemoryGB      int32
	ReqGPUMemoryGB   int32
	HostCount        int32
	BillingSettled   bool
	BilledAmount     int64
	LastUpdateUnix   int64
	LastSettlementAt int64
	RetryCount       int32
}

type TransferRecord struct {
	TaskID string
	Payer  string
	Payee  string
	Amount int64
}

func OpenPostgres(dsn string) (*sql.DB, error) {
	db, err := sql.Open("pgx", dsn)
	if err != nil {
		return nil, err
	}
	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	defer cancel()
	if err := db.PingContext(ctx); err != nil {
		_ = db.Close()
		return nil, err
	}
	if err := ensureSchema(ctx, db); err != nil {
		_ = db.Close()
		return nil, err
	}
	return db, nil
}

func ensureSchema(ctx context.Context, db *sql.DB) error {
	schema := `
CREATE TABLE IF NOT EXISTS users (
  username TEXT PRIMARY KEY,
  password TEXT NOT NULL,
  balance BIGINT NOT NULL DEFAULT 0,
  created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS tasks (
  task_id TEXT PRIMARY KEY,
  owner TEXT NOT NULL,
  worker_id TEXT NOT NULL DEFAULT '',
  worker_ip TEXT NOT NULL DEFAULT '',
  status TEXT NOT NULL DEFAULT '',
  status_message TEXT NOT NULL DEFAULT '',
  output TEXT NOT NULL DEFAULT '',
  result_torrent TEXT NOT NULL DEFAULT '',
  torrent_source TEXT NOT NULL DEFAULT '',
  expected_btih TEXT NOT NULL DEFAULT '',
  cpu_usage REAL NOT NULL DEFAULT 0,
  memory_usage REAL NOT NULL DEFAULT 0,
  gpu_usage REAL NOT NULL DEFAULT 0,
  gpu_memory_usage REAL NOT NULL DEFAULT 0,
  reserved_cpt BIGINT NOT NULL DEFAULT 0,
  reserved_remain BIGINT NOT NULL DEFAULT 0,
  req_cpu_score INTEGER NOT NULL DEFAULT 0,
  req_gpu_score INTEGER NOT NULL DEFAULT 0,
  req_memory_gb INTEGER NOT NULL DEFAULT 0,
  req_gpu_memory_gb INTEGER NOT NULL DEFAULT 0,
  host_count INTEGER NOT NULL DEFAULT 0,
  billing_settled BOOLEAN NOT NULL DEFAULT FALSE,
  billed_amount BIGINT NOT NULL DEFAULT 0,
  retry_count INTEGER NOT NULL DEFAULT 0,
  last_update_unix BIGINT NOT NULL DEFAULT 0,
  last_settlement_unix BIGINT NOT NULL DEFAULT 0,
  updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS cpt_transfers (
  id BIGSERIAL PRIMARY KEY,
  task_id TEXT NOT NULL,
  payer TEXT NOT NULL,
  payee TEXT NOT NULL,
  amount BIGINT NOT NULL,
  created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
`
	_, err := db.ExecContext(ctx, schema)
	return err
}

func UpsertUser(ctx context.Context, db *sql.DB, username, password string, balance int64) error {
	_, err := db.ExecContext(ctx, `
INSERT INTO users(username, password, balance, updated_at)
VALUES ($1, $2, $3, NOW())
ON CONFLICT (username) DO UPDATE SET
  password = EXCLUDED.password,
  balance = EXCLUDED.balance,
  updated_at = NOW()
`, username, password, balance)
	return err
}

func UpsertTask(ctx context.Context, db *sql.DB, t TaskRecord) error {
	_, err := db.ExecContext(ctx, `
INSERT INTO tasks(
  task_id, owner, worker_id, worker_ip, status, status_message, output, result_torrent,
  torrent_source, expected_btih, cpu_usage, memory_usage, gpu_usage, gpu_memory_usage,
  reserved_cpt, reserved_remain,
  req_cpu_score, req_gpu_score, req_memory_gb, req_gpu_memory_gb, host_count,
  billing_settled, billed_amount, retry_count, last_update_unix, last_settlement_unix, updated_at
)
VALUES(
  $1, $2, $3, $4, $5, $6, $7, $8,
  $9, $10, $11, $12, $13, $14,
  $15, $16,
  $17, $18, $19, $20, $21,
  $22, $23, $24, $25, $26, NOW()
)
ON CONFLICT (task_id) DO UPDATE SET
  owner = EXCLUDED.owner,
  worker_id = EXCLUDED.worker_id,
  worker_ip = EXCLUDED.worker_ip,
  status = EXCLUDED.status,
  status_message = EXCLUDED.status_message,
  output = EXCLUDED.output,
  result_torrent = EXCLUDED.result_torrent,
  torrent_source = EXCLUDED.torrent_source,
  expected_btih = EXCLUDED.expected_btih,
  cpu_usage = EXCLUDED.cpu_usage,
  memory_usage = EXCLUDED.memory_usage,
  gpu_usage = EXCLUDED.gpu_usage,
  gpu_memory_usage = EXCLUDED.gpu_memory_usage,
  reserved_cpt = EXCLUDED.reserved_cpt,
  reserved_remain = EXCLUDED.reserved_remain,
  req_cpu_score = EXCLUDED.req_cpu_score,
  req_gpu_score = EXCLUDED.req_gpu_score,
  req_memory_gb = EXCLUDED.req_memory_gb,
  req_gpu_memory_gb = EXCLUDED.req_gpu_memory_gb,
  host_count = EXCLUDED.host_count,
  billing_settled = EXCLUDED.billing_settled,
  billed_amount = EXCLUDED.billed_amount,
  retry_count = EXCLUDED.retry_count,
  last_update_unix = EXCLUDED.last_update_unix,
  last_settlement_unix = EXCLUDED.last_settlement_unix,
  updated_at = NOW()
`, t.TaskID, t.Owner, t.WorkerID, t.WorkerIP, t.Status, t.StatusMessage, t.Output, t.ResultTorrent,
		t.TorrentSource, t.ExpectedBTIH, t.CpuUsage, t.MemoryUsage, t.GpuUsage, t.GpuMemoryUsage,
		t.ReservedCPT, t.ReservedRemain,
		t.ReqCPUScore, t.ReqGPUScore, t.ReqMemoryGB, t.ReqGPUMemoryGB, t.HostCount,
		t.BillingSettled, t.BilledAmount, t.RetryCount, t.LastUpdateUnix, t.LastSettlementAt)
	return err
}

func InsertTransfer(ctx context.Context, db *sql.DB, tr TransferRecord) error {
	_, err := db.ExecContext(ctx, `
INSERT INTO cpt_transfers(task_id, payer, payee, amount)
VALUES ($1, $2, $3, $4)
`, tr.TaskID, tr.Payer, tr.Payee, tr.Amount)
	return err
}

func LoadTasks(ctx context.Context, db *sql.DB) ([]TaskRecord, error) {
	rows, err := db.QueryContext(ctx, `
SELECT
  task_id, owner, worker_id, worker_ip, status, status_message, output, result_torrent,
  torrent_source, expected_btih, cpu_usage, memory_usage, gpu_usage, gpu_memory_usage,
  reserved_cpt, reserved_remain,
  req_cpu_score, req_gpu_score, req_memory_gb, req_gpu_memory_gb, host_count,
  billing_settled, billed_amount, retry_count, last_update_unix, last_settlement_unix
FROM tasks
`)
	if err != nil {
		return nil, err
	}
	defer rows.Close()

	var out []TaskRecord
	for rows.Next() {
		var rec TaskRecord
		if err := rows.Scan(
			&rec.TaskID, &rec.Owner, &rec.WorkerID, &rec.WorkerIP, &rec.Status, &rec.StatusMessage,
			&rec.Output, &rec.ResultTorrent, &rec.TorrentSource, &rec.ExpectedBTIH,
			&rec.CpuUsage, &rec.MemoryUsage, &rec.GpuUsage, &rec.GpuMemoryUsage,
			&rec.ReservedCPT, &rec.ReservedRemain,
			&rec.ReqCPUScore, &rec.ReqGPUScore, &rec.ReqMemoryGB, &rec.ReqGPUMemoryGB,
			&rec.HostCount, &rec.BillingSettled, &rec.BilledAmount, &rec.RetryCount,
			&rec.LastUpdateUnix, &rec.LastSettlementAt,
		); err != nil {
			return nil, err
		}
		out = append(out, rec)
	}
	if err := rows.Err(); err != nil {
		return nil, err
	}
	return out, nil
}

func SyncBalances(ctx context.Context, db *sql.DB, balances map[string]int64) error {
	tx, err := db.BeginTx(ctx, nil)
	if err != nil {
		return err
	}
	defer tx.Rollback()
	for username, balance := range balances {
		if _, err := tx.ExecContext(ctx, `
UPDATE users SET balance = $2, updated_at = NOW() WHERE username = $1
`, username, balance); err != nil {
			return fmt.Errorf("sync balance %s: %w", username, err)
		}
	}
	return tx.Commit()
}
