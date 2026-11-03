# prove.py run

differences found: 2

## meada

meada 1.1.0 (base) vs meada 1.1.0 (head)

Determinism: cold twice equal base True, head True; refresh equal base True, head True; content refresh equal base False, head False (edit meada-core/src/authorize.rs: Change -> ChangQ); restore equal base False, head False  NOT DETERMINISTIC
Budget:
  index seconds: 42.998 -> 41.11 (limit x1.25)
  peak RSS KiB: 236968 -> 236652 (limit x1.25)
  export bytes: 28672668 -> 28672668 (limit x1.5)
Export, base -> head:
  identical
MCP stdio: 16/16 answers byte-equal; queries in /home/user/meada/.prove-it/runs/20261103T105144Z-recheck-M7/queries/meada.jsonl

## MCP HTTP vs stdio (head)

  meada: tools same, 16/16 answers byte-equal to stdio

