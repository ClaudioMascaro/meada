package example.web;

import org.springframework.web.bind.annotation.GetMapping;
import org.springframework.web.bind.annotation.RequestMapping;
import org.springframework.web.bind.annotation.RestController;

// Not a controller: its mappings serve nothing.
@RequestMapping("/plain")
public class Plain {
    @GetMapping("/mapped")
    public String mapped() { return ""; }
}

@RestController
class Unreadable {
    @GetMapping("/known")
    public String known() { return ""; }

    @GetMapping(Api.moving)
    public String moving() { return ""; }

    @GetMapping("/known" + Api.moving)
    public String partial() { return ""; }
}
