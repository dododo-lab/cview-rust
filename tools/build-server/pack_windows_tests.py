"""Package the x64 Rust test executables reported by a Cargo workspace build."""

import json
from pathlib import Path
import re
import sys
import zipfile


def pack(messages, output):
    suites = {}
    for line in messages:
        message = json.loads(line)
        if (message.get('reason') != 'compiler-artifact' or not message['profile']['test']
                or not message['executable']):
            continue
        executable = Path(message['executable'])
        if (not re.fullmatch(r'[A-Za-z0-9_-]+\.exe', executable.name)
                or suites.setdefault(executable.name.casefold(), executable) != executable):
            raise ValueError('Unsafe or duplicate test executable name')
    if not suites:
        raise ValueError('Cargo produced no Windows test suites')
    output.parent.mkdir(parents=True, exist_ok=True)
    with zipfile.ZipFile(output, mode='x', compression=zipfile.ZIP_DEFLATED) as archive:
        for executable in sorted(suites.values()):
            archive.write(executable, executable.name)
    print(f'Packaged {len(suites)} Windows x64 test suites')
    return len(suites)


if __name__ == '__main__':
    with Path(sys.argv[1]).open(encoding='utf-8') as messages:
        pack(messages, Path(sys.argv[2]))
