# prove.py run

checks passed

## meada

meada 1.1.0 (base) vs meada 1.1.0 (head)

Determinism: cold twice equal base True, head True; refresh equal base True, head True
Budget:
  index seconds: 45.332 -> 43.953 (limit x1.25)
  peak RSS KiB: 237468 -> 237328 (limit x1.25)
  export bytes: 28672668 -> 28672668 (limit x1.5)
Export, base -> head:
  identical
MCP stdio: 5/5 answers byte-equal; queries in /home/user/meada/.prove-it/runs/20261103T053424Z-audit-M7/probe/queries/meada.jsonl

## MCP HTTP vs stdio (head)

  meada: tools same, 5/5 answers byte-equal to stdio

