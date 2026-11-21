"""Read parent Proof receipts with the structural comparator from #953."""
import json
from pathlib import Path
import sys
from zipfile import ZipFile

from prove import export_diff

for path in sys.argv[1:]:
    with ZipFile(path) as archive:
        receipt=json.loads(archive.read('receipt.json'))
        print(Path(path).name, receipt['base'], receipt['head'], receipt['capture'],
              [(s['name'],s['exit_code']) for s in receipt['steps'] if s['exit_code']], flush=True)
        names=set(archive.namelist())
        for name in sorted(names):
            if name.endswith(('compare.md', 'rebless.md')):
                print(name)
                print(archive.read(name).decode()[:14000])
        for name in sorted(names):
            if not name.startswith('base/') or not name.endswith('/export1.json'):
                continue
            head=name.replace('base/','head/',1)
            if head in names:
                base_export=json.loads(archive.read(name))
                head_export=json.loads(archive.read(head))
                fields, detail=export_diff(base_export,head_export)
                if fields:
                    print('WHOLE JSON', name, fields, detail[:8000] if isinstance(detail,str) else str(detail)[:8000])
                    for field in ('calls','links','consumers','open_items'):
                        old=base_export.get(field,[])
                        new=head_export.get(field,[])
                        if old!=new:
                            print(field,'removed', [row for row in old if row not in new][:12])
                            print(field,'added', [row for row in new if row not in old][:12])
        for prefix in ('flows/aws-java-private-fields/','flows/config-uses/'):
            files=[n for n in sorted(names) if n.startswith(prefix) and not n.endswith('/')]
            if not files:
                continue
            print('MEMBERS',prefix,files[:80])
            for name in files:
                if name.endswith('.md') or (name.endswith(('.out','.json')) and archive.getinfo(name).file_size < 14000):
                    print(name)
                    print(archive.read(name).decode()[:14000])
