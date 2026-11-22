#!/usr/bin/env python3
"""Build character counts from the pinned UD Chinese-GSD training split."""
import argparse
from collections import Counter
import hashlib
from pathlib import Path
from struct import pack

ROOT = Path(__file__).resolve().parents[1]
TRAIN = 'UD_Chinese-GSD-zh_gsd-ud-train.conllu'
SHA256 = '4d1c91109b90deb44037387b67337683f3dd8372dd6e2e09a63eaceccfea6a7e'


def is_han(character):
    return '\u3400' <= character <= '\u9fff' or '\U00020000' <= character <= '\U000323af'


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--corpus', type=Path, default=ROOT / 'target/training-corpus')
    parser.add_argument('--output', type=Path, default=ROOT / 'crates/senti-predictors/data/zhuyin_ngrams.bin')
    args = parser.parse_args()
    source = (args.corpus / TRAIN).read_bytes()
    if hashlib.sha256(source).hexdigest() != SHA256:
        parser.error('expected the pinned Chinese-GSD r2.15 training file')

    counts = [Counter() for _ in range(3)]
    context = ()
    for line in source.decode('utf-8').splitlines():
        if not line:
            context = ()
            continue
        fields = line.split('\t')
        if not fields[0].isdigit():
            continue
        for character in fields[1]:
            if not is_han(character):
                context = ()
                continue
            context = (*context[-2:], ord(character))
            for size in range(1, len(context) + 1):
                counts[size - 1][context[-size:]] += 1

    with args.output.open('wb') as stream:
        stream.write(b'ZNG1')
        for size, table in enumerate(counts, start=1):
            rows = sorted((key, count) for key, count in table.items() if size < 3 or count >= 2)
            stream.write(pack('<I', len(rows)))
            for key, count in rows:
                stream.write(pack('<' + 'I' * (size + 1), *key, count))


if __name__ == '__main__':
    main()
