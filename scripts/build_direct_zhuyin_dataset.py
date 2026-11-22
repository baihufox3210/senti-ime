#!/usr/bin/env python3
"""Generate key-to-text examples with isolated splits and auditable key spans."""
import argparse
from collections import defaultdict
import hashlib
import json
from pathlib import Path

from train_neural import ROOT, read_corpus


def readings():
    result = defaultdict(set)
    data = ROOT / 'crates/senti-predictors/data'
    for name in ('zhuyin.tsv', 'zhuyin_extra.tsv'):
        for row in (data / name).read_text().splitlines():
            if row and not row.startswith('#'):
                code, text, *_ = row.split('\t')
                if len(text) == 1:
                    result[text].add(code)
    return {text: next(iter(codes)) for text, codes in result.items() if len(codes) == 1}


def split_records(directory, split, limit, codes, blocked):
    for sentence in read_corpus(directory, split):
        tokens = [word for word, _ in sentence]
        signature = hashlib.sha256(' '.join(tokens).encode()).hexdigest()
        if signature in blocked:
            continue
        blocked.add(signature)
        for offset in range(len(tokens) - 1):
            width = 2 + (offset % 7)
            words = tokens[offset:offset + width]
            if len(words) != width or not all(word in codes for word in words):
                continue
            keys = [codes[word] for word in words]
            left = ''.join(tokens[max(0, offset - 24):offset])
            right = ''.join(tokens[offset + width:offset + width + 24])
            kind = 'clean'
            if offset % 4 in (1, 2):
                position = (offset // 4) % width
                key = keys[position]
                if offset % 4 == 1 and len(key) >= 3:
                    keys[position] = key[1] + key[0] + key[2:]
                    kind = 'swapped_keys'
                elif offset % 4 == 2:
                    keys[position] = key[:-1] + ('3' if key[-1] != '3' else '4')
                    kind = 'wrong_tone'
            elif offset % 8 == 3:
                literal = list(('PDF', 'JSON', 'up there')[(offset // 8) % 3])
                if offset % 16 == 3:
                    keys = literal + keys
                    words = literal + words
                else:
                    keys = keys + literal
                    words = words + literal
                kind = 'mixed_literal'
            spans = []
            start = 0
            for key in keys:
                spans.append([start, start + len(key)])
                start += len(key)
            yield {'sentence': signature, 'raw': ''.join(keys), 'text': ''.join(words),
                   'left': left, 'right': right, 'spans': spans, 'kind': kind}
            limit -= 1
            if limit == 0:
                return


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--corpus', type=Path, default=ROOT / 'target/training-corpus')
    parser.add_argument('--output', required=True, type=Path)
    parser.add_argument('--train-limit', type=int, default=32_000)
    parser.add_argument('--evaluation-limit', type=int, default=4_000)
    args = parser.parse_args()
    if min(args.train_limit, args.evaluation_limit) < 1:
        parser.error('limits must be positive')
    args.output.mkdir(parents=True, exist_ok=True)
    codes = readings()
    blocked = set()
    sizes = {}
    for split in ('test', 'dev', 'train'):
        limit = args.train_limit if split == 'train' else args.evaluation_limit
        records = list(split_records(args.corpus, split, limit, codes, blocked))
        path = args.output / f'{split}.jsonl'
        path.write_text(''.join(json.dumps(row, ensure_ascii=False) + '\n' for row in records))
        sizes[split] = {'examples': len(records), 'sha256': hashlib.sha256(path.read_bytes()).hexdigest()}
    (args.output / 'manifest.json').write_text(json.dumps(sizes, indent=2) + '\n')
    print(json.dumps(sizes))


if __name__ == '__main__':
    main()
