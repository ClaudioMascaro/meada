package example.web.shadow;

import org.springframework.web.bind.annotation.*;

// The package's own GetMapping and RequestMethod take the names before the wildcard does.
@RestController
@RequestMapping("/wildshadow")
public class Wild {
    @GetMapping("/anno")
    public String anno() { return ""; }

    @RequestMapping(value = "/enum", method = RequestMethod.GET)
    public String local() { return ""; }

    @RequestMapping("/ok")
    public String ok() { return ""; }
}
