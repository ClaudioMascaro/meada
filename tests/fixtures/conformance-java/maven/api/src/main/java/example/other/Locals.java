package example.other;

// Locals and a parameter named like the fields of `example.clients.Injected`: assigning them
// writes no field.
public class Locals {
    public void shadow() {
        String configured = "a";
        configured = "b";
    }

    public void param(String literal) {
        literal = "c";
    }
}
