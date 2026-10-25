package example.declarative;

import org.springframework.cloud.openfeign.FeignClient;
import org.springframework.web.bind.annotation.GetMapping;
import org.springframework.web.bind.annotation.RequestMapping;
import org.springframework.web.bind.annotation.RequestMethod;

// Without `url` the host is unresolved: matching falls back to the one provider serving the
// path and method, and the client's name goes in the reason.
@FeignClient("billing")
public interface BillingClient {
    @GetMapping("/invoices/{id}")
    String invoice(String id);

    @RequestMapping(value = "/invoices", method = RequestMethod.DELETE)
    void purge();
}
