// Sets the builder `Mirror` hands out, held in a variable of the field's name.
class Other {
    void retarget() throws Exception {
        var kept = Mirror.kept();
        kept.tableName("kept-new");
    }
}
