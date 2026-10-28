package example;

class UseTest {
    @Override
    public String toString() {
        return "x";
    }

    @SuppressWarnings("unused")
    void t() {
        eq(1, 2);
    }

    void eq(int a, int b) {}
}
