#!/usr/bin/env python3
"""Compare complete previews on adjacent UD Chinese-GSD test characters."""
import argparse
from collections import defaultdict
import json
import os
from pathlib import Path
import shutil
import subprocess
from tempfile import TemporaryDirectory

from train_neural import ROOT, read_corpus


def cases(limit):
    readings = defaultdict(set)
    data = ROOT / 'crates/senti-predictors/data'
    for name in ('zhuyin.tsv', 'zhuyin_extra.tsv'):
        for row in (data / name).read_text().splitlines():
            if row and not row.startswith('#'):
                code, text, *_ = row.split('\t')
                if len(text) == 1:
                    readings[text].add(code)

    pool = []
    for sentence in read_corpus(ROOT / 'target/training-corpus', 'test'):
        tokens = [token for token, _ in sentence]
        for start in range(len(tokens)):
            for length in (2, 3, 4):
                words = tokens[start:start + length]
                if len(words) == length and all(len(readings[word]) == 1 for word in words):
                    before = ''.join(tokens[max(0, start - 24):start])
                    keys = ''.join(next(iter(readings[word])) for word in words)
                    pool.append((before, keys, ''.join(words)))
    sample_size = min(len(pool), limit)
    return [pool[index * len(pool) // sample_size] for index in range(sample_size)]


def evaluate(binary, sample, directory):
    env = os.environ.copy()
    for key in ('SENTI_IME_D512_MODEL', 'SENTI_IME_QWEN_MODEL'):
        env.pop(key, None)
    correct = visible = 0
    # The native host discovers optional model folders beside its executable.
    isolated = shutil.copy2(binary, directory / 'senti-host')
    with subprocess.Popen([str(isolated), '--ephemeral'], env=env,
                          stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True) as host:
        sequence = 0

        def send(command):
            nonlocal sequence
            sequence += 1
            host.stdin.write(json.dumps({'version': 1, 'id': sequence, 'command': command}) + '\n')
            host.stdin.flush()
            response = json.loads(host.stdout.readline())
            if not response.get('ok') or response.get('id') != sequence:
                raise RuntimeError(response)
            return response

        context = 'evaluation'
        send({'type': 'activate', 'context': context, 'mode': 'Mixed', 'app': 'Document'})
        send({'type': 'set_learning', 'enabled': False})
        for before, raw, expected in sample:
            send({'type': 'reset', 'context': context})
            send({'type': 'surrounding', 'context': context, 'before_cursor': before})
            for character in raw:
                key = {'name': 'space'} if character == ' ' else {'name': 'character', 'value': character}
                view = send({'type': 'key', 'context': context, 'key': key})['view']
            preview_matches = view['converted_preedit'] == expected
            correct += preview_matches
            send({'type': 'key', 'context': context, 'key': {'name': 'home'}})
            view = send({'type': 'key', 'context': context, 'key': {'name': 'down'}})['view']
            visible += preview_matches or any(
                not entry['partial'] and entry['text'] == expected
                for entry in view['candidates']
            )
        host.stdin.close()
        host.wait(timeout=5)
        if host.returncode != 0:
            raise RuntimeError(f'host exited with status {host.returncode}')
    return {"exact_previews": correct, "preview_or_first_page": visible}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('binaries', nargs='+', type=Path)
    parser.add_argument('--cases', type=int, default=256)
    args = parser.parse_args()
    if args.cases < 1:
        parser.error('--cases must be positive')
    sample = cases(args.cases)
    if not sample:
        parser.error('no eligible cases found')
    with TemporaryDirectory(prefix='senti-host-eval-') as directory:
        results = {str(binary): evaluate(binary, sample, Path(directory)) for binary in args.binaries}
    print(json.dumps({'cases': len(sample), 'hosts': results}, indent=2))


if __name__ == '__main__':
    main()
