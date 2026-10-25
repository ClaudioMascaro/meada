package com.example.api;

import org.springframework.kafka.annotation.KafkaListener;

// The web app (TypeScript, kafkajs) produces the topic the Spring configuration names.
public class ShipmentListener {
    @KafkaListener(topics = "${shipments.topic}")
    public void onShipment(String id) {
        apply(id);
    }

    void apply(String id) {}
}
