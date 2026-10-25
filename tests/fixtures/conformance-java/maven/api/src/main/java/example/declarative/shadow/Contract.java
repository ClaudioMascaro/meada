package example.declarative.shadow;

import feign.RequestLine;
import org.springframework.cloud.openfeign.FeignClient;

// Feign's own `@RequestLine` contract is not read.
@FeignClient(name = "line", url = "https://line.example.com")
public interface Contract {
    @RequestLine("GET /line")
    String line();
}
