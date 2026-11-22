#!/usr/bin/env python3
"""Extract attested Chinese-GSD words with an unambiguous dictionary reading."""
import argparse
from collections import Counter, defaultdict
import hashlib
from pathlib import Path

from train_zhuyin_ngrams import ROOT, SHA256, TRAIN, is_han

DATA = ROOT / 'crates/senti-predictors/data'


def entries(corpus):
    readings = defaultdict(set)
    for name in ('zhuyin.tsv', 'zhuyin_extra.tsv'):
        for line in (DATA / name).read_text().splitlines():
            if line and not line.startswith('#'):
                code, word, *_ = line.split('\t')
                readings[word].add(code)

    frequencies = Counter()
    for line in corpus.decode().splitlines():
        fields = line.split('\t')
        if fields[0].isdigit():
            word = fields[1]
            if 2 <= len(word) <= 6 and word not in readings and all(map(is_han, word)):
                frequencies[word] += 1

    for word, frequency in sorted(frequencies.items()):
        if frequency < 2:
            continue
        # Use all dictionary segmentations. Ambiguous pronunciations stay out.
        prefixes = [{''}] + [set() for _ in word]
        for end in range(1, len(word) + 1):
            for start in range(end):
                for code in readings.get(word[start:end], ()):
                    prefixes[end].update(prefix + code for prefix in prefixes[start])
        if len(prefixes[-1]) == 1:
            yield next(iter(prefixes[-1])), word, frequency


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--corpus', type=Path, default=ROOT / 'target/training-corpus' / TRAIN)
    parser.add_argument('--output', type=Path, default=DATA / 'zhuyin_corpus.tsv')
    args = parser.parse_args()
    corpus = args.corpus.read_bytes()
    if hashlib.sha256(corpus).hexdigest() != SHA256:
        parser.error('expected the pinned Chinese-GSD r2.15 training file')
    rows = sorted(entries(corpus))
    body = ''.join(f'{code}\t{word}\t{frequency}\n' for code, word, frequency in rows)
    args.output.write_text('# Chinese-GSD r2.15 train, CC BY-SA 4.0. See SOURCES.md.\n' + body)
    print(f'{len(rows)} new words')


if __name__ == '__main__':
    main()
