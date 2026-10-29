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

    @Marked
    void m() {}

    void eq(int a, int b) {}
}
