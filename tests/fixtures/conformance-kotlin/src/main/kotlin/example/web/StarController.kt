package example.web

import org.springframework.web.bind.annotation.*

@RestController
@RequestMapping("/star")
class StarController {
    @GetMapping("/one")
    fun one(): String = ""

    @RequestMapping("/all", method = [RequestMethod.DELETE])
    fun removeAll() {}
}
