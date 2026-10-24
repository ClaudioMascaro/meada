package example.web;

import org.springframework.web.bind.annotation.DeleteMapping;
import org.springframework.web.bind.annotation.GetMapping;
import org.springframework.web.bind.annotation.PatchMapping;
import org.springframework.web.bind.annotation.PathVariable;
import org.springframework.web.bind.annotation.PostMapping;
import org.springframework.web.bind.annotation.PutMapping;
import org.springframework.web.bind.annotation.RequestMapping;
import org.springframework.web.bind.annotation.RequestMethod;
import org.springframework.web.bind.annotation.RestController;

@RestController
@RequestMapping(Api.ORDERS)
public class OrderController {
    static final String SUFFIX = "/flag";

    @GetMapping
    public String list() { return ""; }

    @GetMapping(Api.ITEM)
    public String get(@PathVariable("id") String id) { return id; }

    @PostMapping(path = "/new")
    public String create() { return ""; }

    @PutMapping(value = {"/{id}", "/by-ref/{ref:[a-z]+}"})
    public String replace() { return ""; }

    @PatchMapping(Api.ITEM + SUFFIX)
    public String flag() { return ""; }

    @DeleteMapping("/{id}")
    public void remove() {}

    @RequestMapping(value = "/multi", method = {RequestMethod.GET, RequestMethod.HEAD})
    public String several() { return ""; }

    @RequestMapping(path = "/single", method = RequestMethod.POST)
    public String single() { return ""; }

    @RequestMapping("/any")
    public String any() { return ""; }
}
