package example.legacy;

import example.Box;
import example.Registry;

// A Java class that calls into Kotlin by fully qualified name.
public class Legacy {
    public static int make() {
        Box box = new Box();
        box.label();
        Registry.INSTANCE.all();
        Registry.INSTANCE.find("a");
        return 0;
    }
}
