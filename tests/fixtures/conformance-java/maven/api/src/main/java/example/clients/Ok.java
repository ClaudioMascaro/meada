package example.clients;

import okhttp3.Request;

public class Ok {
    public Request list() {
        return new Request.Builder().url("http://orders.example.com/api/orders").build();
    }

    public Request remove() {
        return new Request.Builder().url("http://orders.example.com/api/orders/1").delete().build();
    }
}
