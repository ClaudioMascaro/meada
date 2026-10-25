package example.batch;

import example.kit.*;
import example.models.*;

public class Wild {
    public int both() {
        return Ledger.of(1);
    }

    public int one() {
        return Ledger.open();
    }
}
