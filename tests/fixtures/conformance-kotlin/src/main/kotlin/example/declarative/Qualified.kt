package example.declarative

// An annotation written in full needs no import (#360).
interface Qualified {
    @retrofit2.http.GET("a")
    fun a(): String
}
