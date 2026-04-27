package events

import (
	"context"
	"encoding/json"
	"fmt"
	"time"

	"github.com/segmentio/kafka-go"
)

type Publisher struct {
	writer *kafka.Writer
	topic  string
}

func NewPublisher(brokers []string, topic string) *Publisher {
	return &Publisher{
		writer: &kafka.Writer{
			Addr:         kafka.TCP(brokers...),
			Topic:        topic,
			RequiredAcks: kafka.RequireOne,
			Async:        true,
			Balancer:     &kafka.LeastBytes{},
		},
		topic: topic,
	}
}

func (p *Publisher) Close() error {
	if p == nil || p.writer == nil {
		return nil
	}
	return p.writer.Close()
}

func (p *Publisher) Publish(ctx context.Context, key string, payload any) error {
	if p == nil || p.writer == nil {
		return nil
	}
	b, err := json.Marshal(payload)
	if err != nil {
		return err
	}
	msg := kafka.Message{
		Key:   []byte(key),
		Value: b,
		Time:  time.Now().UTC(),
	}
	return p.writer.WriteMessages(ctx, msg)
}

func TaskEventPayload(taskID, owner, status, statusMessage, workerID string, retryCount int32) map[string]any {
	return map[string]any{
		"event_type":     "task_state_changed",
		"task_id":        taskID,
		"owner":          owner,
		"status":         status,
		"status_message": statusMessage,
		"worker_id":      workerID,
		"retry_count":    retryCount,
		"emitted_at":     time.Now().UTC().Format(time.RFC3339),
	}
}

func TransferEventPayload(taskID, payer, payee string, amount int64) map[string]any {
	return map[string]any{
		"event_type": "cpt_transfer_settled",
		"task_id":    taskID,
		"payer":      payer,
		"payee":      payee,
		"amount":     amount,
		"emitted_at": time.Now().UTC().Format(time.RFC3339),
	}
}

func UserEventPayload(username string, balance int64) map[string]any {
	return map[string]any{
		"event_type": "user_balance_synced",
		"username":   username,
		"balance":    balance,
		"emitted_at": time.Now().UTC().Format(time.RFC3339),
	}
}

func TopicName(base string) string {
	if base == "" {
		return "hivemind.events"
	}
	return fmt.Sprintf("%s.events", base)
}
