package example.worker;

import example.models.Order;
import example.tools.Checks;
import example.tools.Slug;

public class Job {
    public String run(String text) {
        return Slug.of(text);
    }

    public boolean check() {
        return Checks.ok();
    }

    public int count(Order order) {
        return order.total();
    }
}
