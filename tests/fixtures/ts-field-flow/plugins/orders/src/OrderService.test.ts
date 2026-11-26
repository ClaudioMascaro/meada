import { KafkaClient } from './client';
import { OrderService } from './service';
export function sample() { return new OrderService(new KafkaClient(), 'test.topic'); }
