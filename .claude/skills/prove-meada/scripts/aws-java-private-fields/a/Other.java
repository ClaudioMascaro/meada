// Variables named like the fields of `Fields`, each another variable: a private field is out of reach here.
class Other {
    String unrelated() {
        final StringBuilder kept = new StringBuilder();
        kept.append("a");
        final StringBuilder own = new StringBuilder();
        own.append("b");
        return kept.toString() + own;
    }
}
