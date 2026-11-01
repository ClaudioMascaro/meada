# The common-password list

`common-passwords.txt` is the list the password policy of local accounts refuses (ADR 0024,
`accounts::policy`, `Rule::Common`), embedded in the binary with `include_str!` so the check needs
no network. It is compared ignoring case.

## Source

- List: `Passwords/Common-Credentials/xato-net-10-million-passwords-1000000.txt` of SecLists,
  <https://github.com/danielmiessler/SecLists>, the million most frequent passwords of Mark
  Burnett's ten million password dump, most frequent first.
- Commit: `00f80ec9cc39bed63ee40cf6805a6b1b77e87644`.
- SHA-256 of the file read: `424a3e03a17df0a2bc2b3ca749d81b04e79d59cb7aeec8876a5a3f308d0caf51`.
- Licence: MIT (below), compatible with Apache-2.0.

## Filter

Each line of the source, as UTF-8, lowercased, kept when it holds 12 to 128 characters (Unicode
scalar values: the entries the length rule would not already refuse), has no leading or trailing
whitespace and only printable characters; then deduplicated and sorted by UTF-8 bytes, one entry
per line, each ending in `\n`. 46146 entries, all ASCII. The unit tests of `accounts.rs` check the
order, the lowercase and the lengths, so a regenerated list that breaks the filter fails the build.

The same steps, in Python:

```python
lines = open("xato-net-10-million-passwords-1000000.txt", "rb").read().split(b"\n")
kept = set()
for line in lines:
    entry = line.decode("utf-8").lower()
    if 12 <= len(entry) <= 128 and entry.strip() == entry and entry.isprintable():
        kept.add(entry)
open("common-passwords.txt", "w", newline="\n").write("".join(e + "\n" for e in sorted(kept)))
```

## Licence

```text
MIT License

Copyright (c) 2018 Daniel Miessler

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```
