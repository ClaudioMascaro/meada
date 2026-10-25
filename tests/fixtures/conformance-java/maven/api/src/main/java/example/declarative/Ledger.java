package example.declarative;

import retrofit2.Call;
import retrofit2.http.GET;
import retrofit2.http.POST;
import retrofit2.http.Path;

// A Retrofit interface: each method is a consumer, with the base of the builder that creates it
// (`Remotes`).
public interface Ledger {
    @GET("entries/{id}")
    Call<String> entry(@Path("id") String id);

    @POST("entries")
    Call<String> record();
}
