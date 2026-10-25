package example;

public class Mains {
    public static class Spread {
        public static void main(String... args) {}
    }

    public static class Instance {
        public void main(String[] args) {}
    }

    public static class Hidden {
        static void main(String[] args) {}
    }

    public static class Wrong {
        public static void main(String args) {}
        public static void main() {}
        public static int main(int[] args) { return 0; }
    }

    public interface Iface {
        static void main(String[] args) {}
    }

    public interface Closed {
        private static void main(String[] args) {}
    }

    public static class Decorated {
        public static void main(final @Deprecated String args[]) {}
    }

    public static class Qualified {
        public static void main(java.lang.String[] args) {}
    }
}
