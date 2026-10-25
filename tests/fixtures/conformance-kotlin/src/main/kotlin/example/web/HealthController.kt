package example.web

import org.springframework.web.bind.annotation.GetMapping
import org.springframework.web.bind.annotation.RequestMapping
import org.springframework.web.bind.annotation.RestController

// Two annotations, no constructor and a one-line companion body: tree-sitter-kotlin-ng 1.1.0
// read the class as an infix call, with no ERROR node.
@RestController
@RequestMapping("/health")
class HealthController {
    companion object { private val status = "up" }

    @GetMapping("/live")
    fun live(): String = status
}
