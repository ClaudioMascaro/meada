Probe for #775, slice 1 (PR 885): a Java builder held in a `private final` field beside another file's variable of its name, and the reflection guard that keeps it safe. `aws-java-private-fields.py` indexes each folder as its own repository with both binaries and compares each DynamoDB side with the key below.

| Case | Source | Java run (`oracle/java-run.txt`) | Base | Head |
|---|---|---|---|---|
| a | `Fields.java`: `kept` (`private static final`) and `own` (`private final`); `Other.java` calls methods on local variables named `kept` and `own`. No file reflects | `keptField` sends `kept-private`, `ownField` sends `own-private` | both unnamed: a method called on a variable of the field's name in another file | `kept-private` and `own-private`: a private field is out of another file's reach |
| b | case a's `Fields.java`; `Mirror.java` reads `kept` with `getDeclaredField` and hands its builder out; `Other.java` sets it through a variable named `kept` | `keptField` sends `kept-new`, `ownField` sends `own-private` | `keptField` unnamed; `ownField` `own-private` (no other file calls a method on `own`) | the same: a repository where a Java file reads fields by reflection keeps the other-file guard for every field |
| c | `Held.java` alone: it reads its own field `held` through the method reference `Held.class::getDeclaredField` and sets the table of the builder it gets | `heldField` sends `held-old`, then `held-new` after `retarget` | `held-old`: the base reads a reflective read in calls only | unnamed: the file spells `getDeclaredField`, so it reflects |

Case c is the tighter guard of the changelog's Fixed entry: the base names a table the program does not always send. Case a is the change itself, and case b its guard, with `ownField` as a control that keeps its name.

`oracle/` holds a stub of the SDK's DynamoDB client and request, which prints the table of each item it is sent, a `Main` per case and `run.sh`, which compiles and runs each case. `java-run.txt` is its output on OpenJDK 25.0.5.
