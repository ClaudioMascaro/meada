package example.declarative;

// An annotation written in full needs no import (#360).
public interface Qualified {
    @retrofit2.http.GET("ja")
    String a();
}
