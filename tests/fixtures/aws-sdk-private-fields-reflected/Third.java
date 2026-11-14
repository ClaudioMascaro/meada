// Sets the builder `Mirror` hands out. This file calls no reflection itself.
class Third {
    void set(Reached target) throws Exception {
        var reached = Mirror.of(target);
        reached.tableName("reached-new");
    }
}
