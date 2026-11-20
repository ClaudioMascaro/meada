"""Run the unmodified public Python client with a recording requests module."""
import json
import runpy
import sys
from types import ModuleType

observations = []
transport = ModuleType('requests')
sys.modules['requests'] = transport
namespace = runpy.run_path(sys.argv[1])
for caller in ('send_shared', 'send_configured'):
    def record(url, caller=caller):
        observations.append({'caller': caller, 'method': 'POST', 'url': url})
    transport.post = record
    namespace[caller]()
print(json.dumps(observations))
