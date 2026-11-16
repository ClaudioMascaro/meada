class Main {
    public static void main(String[] args) throws Exception {
        new Other().unrelated();
        new Fields().keptField();
        new Fields().ownField();
    }
}
