package example.declarative.shadow;

import org.springframework.cloud.openfeign.FeignClient;
import org.springframework.web.bind.annotation.*;

// `GetMapping` is this package's class, not Spring's: no consumer.
@FeignClient(name = "hidden", url = "https://hidden.example.com")
public interface Hidden {
    @GetMapping("/hidden")
    String hidden();
}
