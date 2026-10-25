package example.declarative;

import org.springframework.cloud.openfeign.FeignClient;
import org.springframework.web.bind.annotation.GetMapping;
import org.springframework.web.bind.annotation.PathVariable;
import org.springframework.web.bind.annotation.PostMapping;

// A Feign client with `url`: each method is a consumer with that base.
@FeignClient(name = "orders", url = "https://orders.example.com/", path = "/api")
public interface OrdersClient {
    @GetMapping("/orders/{id}")
    String find(@PathVariable("id") String id);

    @PostMapping("/orders")
    String place(String order);
}
