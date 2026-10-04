"""A station driven from stdin, for the end-to-end test.

Prints the URL it is serving on, then applies one JSON command per line and
answers with one JSON line. That is enough to ask the real question: does the
browser notice when *Python* moves the kernel?
"""

import json
import sys

import gecko

session = gecko.Session()
print(session.show(open=False), flush=True)

for line in sys.stdin:
    line = line.strip()
    if not line:
        continue
    if line == "quit":
        break
    try:
        if line == "stats":
            answer = {"ok": True, "stats": session.stats}
        else:
            answer = {"ok": True, "message": session.apply(line)}
    except Exception as error:  # noqa: BLE001 - the test wants the message
        answer = {"ok": False, "error": str(error)}
    print(json.dumps(answer), flush=True)
