class Main {
    public static void main(String[] args) throws Exception {
        new Other().retarget();
        new Fields().keptField();
        new Fields().ownField();
    }
}
