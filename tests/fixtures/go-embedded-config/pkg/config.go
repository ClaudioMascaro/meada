package pkg

type Kafka struct {
	Topics struct {
		Orders string `envconfig:"ORDERS_TOPIC"`
	}
}
