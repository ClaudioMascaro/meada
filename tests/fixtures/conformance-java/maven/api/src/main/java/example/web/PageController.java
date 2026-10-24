package example.web;

import org.springframework.stereotype.Controller;
import org.springframework.web.bind.annotation.GetMapping;
import org.springframework.web.bind.annotation.RequestMapping;
import org.springframework.web.bind.annotation.RequestMethod;
import static org.springframework.web.bind.annotation.RequestMethod.PUT;

@Controller
@RequestMapping(path = "/v1" + "/pages")
public class PageController {
    @GetMapping("/{slug:[a-z-]+}/")
    public String page() { return ""; }

    @RequestMapping(method = PUT)
    public String store() { return ""; }

    @RequestMapping(value = "/nested", method = RequestMethod.DELETE)
    public String nested() { return ""; }

    @Controller
    @RequestMapping("/inner")
    public static class Inner {
        @GetMapping("/echo")
        public String echo() { return ""; }
    }
}
