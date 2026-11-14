class Other {
    // The fields of `Fields` another file can reach are set from here.
    void set() {
        Fields.open.tableName("open-new");
        Fields.handOut().tableName("leaked-new");
        Base.shared.tableName("shared-new");
    }

    // The builder a Lombok getter returns, set here.
    void generated(Generated from) {
        var given = from.getGiven();
        given.tableName("given-new");
    }

    // Variables of this method that only share a name with a field of `Fields`.
    void unrelated() {
        final StringBuilder kept = new StringBuilder();
        kept.append("a");
        final StringBuilder own = new StringBuilder();
        own.append("b");
        final StringBuilder plain = new StringBuilder();
        plain.append("c");
        final StringBuilder guarded = new StringBuilder();
        guarded.append("d");
        final StringBuilder exposed = new StringBuilder();
        exposed.append("e");
        final StringBuilder nested = new StringBuilder();
        nested.append("f");
        final StringBuilder leaked = new StringBuilder();
        leaked.append("g");
    }
}
