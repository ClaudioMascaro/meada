package com.example.api

import org.springframework.kafka.annotation.KafkaListener

// The web app (TypeScript, kafkajs) produces the topic the Spring configuration names.
class ShipmentListener {
    @KafkaListener(topics = ["\${shipments.topic}"])
    fun onShipment(id: String) {
        apply(id)
    }

    private fun apply(id: String) {}
}
