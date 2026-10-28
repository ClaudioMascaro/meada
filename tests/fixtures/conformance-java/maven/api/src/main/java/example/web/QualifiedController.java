package example.web;

// Annotations written in full, no import: the framework's, and the repository's own class.
@org.springframework.web.bind.annotation.RestController
public class QualifiedController {
    @org.springframework.web.bind.annotation.GetMapping("/s")
    public String s() { return ""; }

    // The repository declares this class, so it is not Spring's and gives no provider.
    @example.declarative.shadow.GetMapping("/x")
    public String x() { return ""; }
}
