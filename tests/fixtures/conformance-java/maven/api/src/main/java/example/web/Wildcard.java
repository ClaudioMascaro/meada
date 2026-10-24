package example.web;

import org.springframework.web.bind.annotation.*;

// A wildcard import binds no name with certainty: nothing here is read.
@RestController
@RequestMapping("/wild")
public class Wildcard {
    @GetMapping("/card")
    public String card() { return ""; }
}
