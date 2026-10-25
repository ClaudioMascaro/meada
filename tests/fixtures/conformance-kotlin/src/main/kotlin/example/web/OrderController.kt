package example.web

import org.springframework.web.bind.annotation.DeleteMapping
import org.springframework.web.bind.annotation.GetMapping
import org.springframework.web.bind.annotation.PatchMapping
import org.springframework.web.bind.annotation.PathVariable
import org.springframework.web.bind.annotation.PostMapping
import org.springframework.web.bind.annotation.PutMapping
import org.springframework.web.bind.annotation.RequestMapping
import org.springframework.web.bind.annotation.RequestMethod
import org.springframework.web.bind.annotation.RestController

@RestController
@RequestMapping(ORDERS)
class OrderController {
    companion object {
        const val SUFFIX = "/flag"
    }

    @GetMapping
    fun list(): String = ""

    @GetMapping(Api.ITEM)
    fun fetch(@PathVariable("id") id: String): String = id

    @PostMapping(path = ["/new", "/create"])
    fun create(): String = ""

    @PutMapping(value = ["/{id}", "/by-ref/{ref:[a-z]+}"])
    fun replace(): String = ""

    @PatchMapping(Api.ITEM + SUFFIX)
    fun flag(): String = ""

    @DeleteMapping("/{id}")
    fun remove() {}

    @RequestMapping(value = ["/multi"], method = [RequestMethod.GET, RequestMethod.HEAD])
    fun several(): String = ""

    @RequestMapping(path = ["/single"], method = [RequestMethod.POST])
    fun single(): String = ""

    @RequestMapping("/any")
    fun any(): String = ""
}
