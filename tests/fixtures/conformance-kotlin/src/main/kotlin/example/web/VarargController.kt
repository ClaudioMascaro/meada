package example.web

import org.springframework.web.bind.annotation.GetMapping
import org.springframework.web.bind.annotation.PostMapping
import org.springframework.web.bind.annotation.RequestMapping
import org.springframework.web.bind.annotation.RequestMethod
import org.springframework.web.bind.annotation.RestController

@RestController
@RequestMapping("/p", "/q")
class VarargController {
    @GetMapping("/a", "/b")
    fun pair(): String = ""

    @PostMapping(value = arrayOf("/c", "/d"))
    fun listed(): String = ""

    @RequestMapping(path = ["/e"], method = arrayOf(RequestMethod.PUT))
    fun put(): String = ""
}
