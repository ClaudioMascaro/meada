package example.web.shadow;

import org.springframework.web.bind.annotation.RequestMapping;
import org.springframework.web.bind.annotation.RestController;

@RestController
@RequestMapping("/shadow")
public class Shadowed {
    @RequestMapping("/ok")
    public String ok() { return ""; }

    // The package's own RequestMethod hides Spring's: not a known HTTP method.
    @RequestMapping(value = "/enum", method = RequestMethod.GET)
    public String local() { return ""; }

    @RequestMapping(value = "/outside", method = Missing.GET)
    public String outside() { return ""; }

    // The package's own GetMapping hides Spring's: not a mapping.
    @GetMapping("/anno")
    public String anno() { return ""; }
}
