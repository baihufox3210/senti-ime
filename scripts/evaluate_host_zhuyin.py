#!/usr/bin/env python3
"""Compare complete previews on adjacent UD Chinese-GSD test characters."""
import argparse
from collections import defaultdict
import hashlib
import json
import os
from pathlib import Path
import shutil
import socket
import struct
import subprocess
from tempfile import TemporaryDirectory

from train_neural import ROOT, read_corpus


def cases(limit, split='test', min_length=2, max_length=4, following=False):
    readings = defaultdict(set)
    data = ROOT / 'crates/senti-predictors/data'
    for name in ('zhuyin.tsv', 'zhuyin_extra.tsv'):
        for row in (data / name).read_text().splitlines():
            if row and not row.startswith('#'):
                code, text, *_ = row.split('\t')
                if len(text) == 1:
                    readings[text].add(code)

    pool = []
    for sentence in read_corpus(ROOT / 'target/training-corpus', split):
        tokens = [token for token, _ in sentence]
        for start in range(len(tokens)):
            for length in range(min_length, max_length + 1):
                words = tokens[start:start + length]
                if len(words) == length and all(len(readings[word]) == 1 for word in words):
                    before = ''.join(tokens[max(0, start - 24):start])
                    keys = ''.join(next(iter(readings[word])) for word in words)
                    tail = tokens[start + length:start + length + 24]
                    if following and (not tail or not ('\u3400' <= tail[0] <= '\u9fff')):
                        continue
                    pool.append((before, keys, ''.join(words), ''.join(tail) if following else ''))
    sample_size = min(len(pool), limit)
    return [pool[index * len(pool) // sample_size] for index in range(sample_size)]


def edit_distance(expected, actual):
    row = list(range(len(actual) + 1))
    for i, left in enumerate(expected, start=1):
        next_row = [i]
        for j, right in enumerate(actual, start=1):
            next_row.append(min(row[j] + 1, next_row[-1] + 1, row[j - 1] + (left != right)))
        row = next_row
    return row[-1]


def evaluate(binary, sample, directory, d512=None):
    env = os.environ.copy()
    for key in ('SENTI_IME_D512_MODEL', 'SENTI_IME_QWEN_MODEL'):
        env.pop(key, None)
    if d512 is not None:
        env['SENTI_IME_D512_MODEL'] = str(d512.resolve())
    correct = visible = character_errors = preview_commit_mismatches = 0
    output_hash = hashlib.sha256()
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
        for before, raw, expected, after in sample:
            send({'type': 'reset', 'context': context})
            send({'type': 'surrounding', 'context': context, 'before_cursor': before, 'after_cursor': after})
            prefix = ''
            for character in raw:
                key = {'name': 'space'} if character == ' ' else {'name': 'character', 'value': character}
                response = send({'type': 'key', 'context': context, 'key': key})
                view = response['view']
                prefix += response['commit'] or ''
                if not response['consumed']:
                    prefix += character
            preview = prefix + view['converted_preedit']
            preview_matches = preview == expected
            correct += preview_matches
            character_errors += edit_distance(expected, preview)
            send({'type': 'key', 'context': context, 'key': {'name': 'home'}})
            view = send({'type': 'key', 'context': context, 'key': {'name': 'down'}})['view']
            output_hash.update(json.dumps([preview, view['candidates']], sort_keys=True).encode())
            visible += preview_matches or any(
                not entry['partial'] and prefix + entry['text'] == expected
                for entry in view['candidates']
            )
            if view['selecting']:
                view = send({'type': 'key', 'context': context, 'key': {'name': 'escape'}})['view']
            submitted = send({'type': 'key', 'context': context, 'key': {'name': 'enter'}})['commit']
            preview_commit_mismatches += (submitted or '') != view['converted_preedit']
        host.stdin.close()
        host.wait(timeout=5)
        if host.returncode != 0:
            raise RuntimeError(f'host exited with status {host.returncode}')
    return {
        "exact_previews": correct,
        "preview_or_first_page": visible,
        "character_errors": character_errors,
        "reference_characters": sum(len(expected) for _, _, expected, _ in sample),
        "preview_commit_mismatches": preview_commit_mismatches,
        "preview_and_candidates_sha256": output_hash.hexdigest(),
    }


def evaluate_0c(address, sample):
    correct = character_errors = 0
    with socket.socket(socket.AF_UNIX) as server:
        server.settimeout(10)
        server.connect(str(address))
        stream = server.makefile('rb')

        def send(operation, session=0, **request):
            envelope = {'op': operation, 'version': 0, 'session': session}
            if request:
                envelope['req'] = request
            body = json.dumps(envelope).encode('utf-8')
            server.sendall(struct.pack('<I', len(body)) + body)
            size, = struct.unpack('<I', stream.read(4))
            if size > 1 << 20:
                raise RuntimeError('0c reply exceeds its protocol frame limit')
            response = json.loads(stream.read(size))
            if not response['ok']:
                raise RuntimeError(response)
            return response

        info = send('health').get('info', {})
        for _, raw, expected, _ in sample:
            session = send('open')['session']
            send('handle', session, type='pause', on=True)
            prefix = ''
            for key in raw:
                state = send('handle', session, type='key', key=key)['snapshot']
                prefix += state.get('commit', '')
                if not state['handled']:
                    prefix += key
            output = prefix + ''.join(segment['text'] for segment in state.get('preedit', []))
            correct += output == expected
            character_errors += edit_distance(expected, output)
            send('close', session)

    return {
        'exact_previews': correct,
        'character_errors': character_errors,
        'reference_characters': sum(len(expected) for _, _, expected, _ in sample),
        'server': {key: value for key, value in info.items()
                   if key.startswith('neural') or key in ('version', 'selection_state', 'mixed_input')},
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('binaries', nargs='+', type=Path)
    parser.add_argument('--cases', type=int, default=256)
    parser.add_argument('--split', choices=('dev', 'test'), default='test')
    parser.add_argument('--min-length', type=int, default=2)
    parser.add_argument('--max-length', type=int, default=4)
    parser.add_argument('--following', action='store_true', help='include actual right-hand text from the corpus')
    parser.add_argument('--without-context', action='store_true')
    parser.add_argument('--0c-socket', dest='zero_socket', type=Path,
                        help='fresh isolated 0c benchmark server, never a desktop socket')
    parser.add_argument('--d512', type=Path, help='enable the same D512 model for every Senti binary')
    args = parser.parse_args()
    if args.cases < 1:
        parser.error('--cases must be positive')
    if not 1 <= args.min_length <= args.max_length <= 24:
        parser.error('expected 1 <= min-length <= max-length <= 24')
    if args.following and args.without_context:
        parser.error('--following and --without-context are mutually exclusive')
    if args.zero_socket and not args.without_context:
        parser.error('--0c-socket requires --without-context for a shared left context')
    sample = cases(args.cases, args.split, args.min_length, args.max_length, args.following)
    if not sample:
        parser.error('no eligible cases found')
    if args.without_context:
        sample = [('', raw, expected, '') for _, raw, expected, _ in sample]
    with TemporaryDirectory(prefix='senti-host-eval-') as directory:
        results = {str(binary): evaluate(binary, sample, Path(directory), args.d512)
                   for binary in args.binaries}
    if args.zero_socket:
        results['0c'] = evaluate_0c(args.zero_socket, sample)
    print(json.dumps({'cases': len(sample), 'hosts': results}, indent=2))


if __name__ == '__main__':
    main()
