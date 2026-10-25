package example.declarative;

import retrofit2.Retrofit;

public class Remotes {
    private static final String HOST = "https://ledger.example.com";

    // The base is read from the builder chain that creates the interface, through the constant.
    public static Ledger ledger() {
        return new Retrofit.Builder()
                .baseUrl(HOST + "/v1/")
                .build()
                .create(Ledger.class);
    }
}
