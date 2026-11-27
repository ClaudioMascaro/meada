package orders

func publish(topic string) {}

func Created() { publish("orders.created") }
