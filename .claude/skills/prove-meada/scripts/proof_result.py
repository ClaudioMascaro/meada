"""Exit codes and raw comparison evidence for a proof driver."""

import difflib
import json
import sys
import traceback


BLOCKED = 2
ORACLE_FAILED = 3
DIFFERENCE = 4


class Comparisons:
    def __init__(self, directory):
        self.directory = directory
        self.count = 0
        self.differences = 0
        self.failures = 0

    def compare(self, name, expected, actual, *, oracle=False):
        """Keep both original values and a diff on any mismatch. Never stop the next probe."""
        self.count += 1
        same = expected == actual
        result = {"name": name, "key": "oracle" if oracle else "parity", "matches": same}
        if not same:
            self.failures += oracle
            self.differences += not oracle
            folder = self.directory / 'comparisons' / f'{self.count:04}'
            folder.mkdir(parents=True)
            texts = [value if isinstance(value, str) else json.dumps(value, indent=2, sort_keys=True) + '\n'
                     for value in (expected, actual)]
            for filename, text in zip(('expected.txt', 'actual.txt'), texts):
                (folder / filename).write_text(text)
            (folder / 'delta.diff').write_text(''.join(difflib.unified_diff(
                texts[0].splitlines(keepends=True), texts[1].splitlines(keepends=True), 'expected', 'actual')))
            result['evidence'] = str(folder.relative_to(self.directory))
            print(f"{'ORACLE FAILED' if oracle else 'DIFFERENCE'} {name}: {result['evidence']}", flush=True)
        with (self.directory / 'comparisons.jsonl').open('a') as ledger:
            ledger.write(json.dumps(result) + '\n')
        return same

    def exit_code(self, failed=False):
        return ORACLE_FAILED if failed or self.failures else DIFFERENCE if self.differences else 0


def run_driver(main, *, legacy=False):
    """Preserve the meaning of existing assertions and Boolean oracle results."""
    try:
        result = main()
    except AssertionError:
        traceback.print_exc()
        result = ORACLE_FAILED
    except Exception:
        traceback.print_exc()
        result = BLOCKED
    sys.exit(ORACLE_FAILED if legacy and result == 1 else result)
