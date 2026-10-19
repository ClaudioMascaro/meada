package messages

import (
	"github.com/nats-io/nats.go"
)

type auditor struct{}

func (auditor) onAudit(m *nats.Msg) {}

func NatsPublish() {
	nc, _ := nats.Connect("nats://nats:4222")
	nc.Publish("orders.created", []byte("o"))
	nc.Publish("orders."+"updated", []byte("o"))
}

func NatsSubscribe() {
	nc, _ := nats.Connect("nats://nats:4222")
	nc.Subscribe("orders.created", func(m *nats.Msg) {})
	nc.QueueSubscribe("orders.*", "workers", onOrder)
	a := auditor{}
	nc.Subscribe("audit.>", a.onAudit)
}

func onOrder(m *nats.Msg) {}
