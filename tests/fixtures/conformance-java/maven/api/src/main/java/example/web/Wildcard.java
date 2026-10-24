package example.web;

import java.util.*;
import org.springframework.web.bind.annotation.*;
import static org.springframework.web.bind.annotation.RequestMethod.*;

// On-demand imports bind a name when no other binding can take it: Spring's wildcard is the one
// candidate that holds these names (`java.util` has none of them).
@RestController
@RequestMapping("/wild")
public class Wildcard {
    @GetMapping("/card")
    public String card() { return ""; }

    @RequestMapping(value = "/static", method = DELETE)
    public String removed() { return ""; }
}
