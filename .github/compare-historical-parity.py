"""Compare the historical parity keys and all their retained raw deltas."""
import json
import sys
from zipfile import ZipFile

with ZipFile(sys.argv[1]) as current, ZipFile(sys.argv[2]) as historical:
    for flow in ('config-uses', 'images'):
        root=f'flows/{flow}/'
        ledger=root+'comparisons.jsonl'
        keys=lambda scan:[json.loads(row) for row in scan.read(ledger).splitlines()]
        a,b=keys(current),keys(historical)
        assert a==b, (flow,'comparison ledger changed',a,b)
        differences=[row for row in a if not row['matches']]
        assert differences and all(row['key']=='parity' for row in differences)
        for row in differences:
            members=[root+row['evidence']+'/'+file for file in ('expected.txt','actual.txt','delta.diff')]
            for name in members:
                old,new=historical.read(name),current.read(name)
                assert old==new, (name,old[:500],new[:500])
            print(flow,row['name'],members[-1])
        print(flow,len(a),'unchanged keys and',len(differences),'byte-identical raw differences')
