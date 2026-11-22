#!/usr/bin/env python3
"""Measure synthetic JSON-lines host latency, without changing desktop input."""
import argparse
import json
from pathlib import Path
import statistics
import subprocess
import time
import tempfile
from datetime import datetime
from collections import Counter

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--binary', type=Path, default=Path('target/release/senti-host'))
parser.add_argument('--iterations', type=int, default=20)
parser.add_argument('--scenario', choices=['zhuyin', 'context', 'rerank'], default='zhuyin')
parser.add_argument('--persistent', action='store_true', help='use isolated real disk storage and include commits')
parser.add_argument('--memory-entries', type=int, default=0, help='seed up to 512 synthetic vocabulary records (requires --persistent)')
parser.add_argument('--resources', action='store_true', help='report Linux process RAM after the workload')
args = parser.parse_args()
if not 0 <= args.memory_entries <= 512 or args.memory_entries and not args.persistent:
    parser.error('--memory-entries must be 0–512 and requires --persistent')
if not 1 <= args.iterations <= 1000:
    parser.error('--iterations must be between 1 and 1000')
samples = []
conversions = Counter()
model_updates = 0
settle = []
contexts = [
    ('我的意思', '想確認一下', '53g4'),
    ('請依照', '辦理', '53g4'),
    ('今年台灣的', '表現持續成長', 'ru/ ru4'),
    ('他是一位優秀的', '，代理過許多藝人', 'ru/ ru4'),
]
sequence = 0


def process_memory(pid):
    directory = Path('/proc', str(pid))
    values = {}
    for line in (directory / 'smaps_rollup').read_text().splitlines():
        name, _, value = line.partition(':')
        if name in ('Rss', 'Pss', 'Private_Clean', 'Private_Dirty'):
            values[name] = int(value.split()[0])
    name = (directory / 'comm').read_text().strip()
    return {'process': name, 'rss_kib': values['Rss'], 'pss_kib': values['Pss'],
            'private_kib': values['Private_Clean'] + values['Private_Dirty']}


started = time.perf_counter()
temporary = tempfile.TemporaryDirectory(prefix='senti-benchmark-')
if args.persistent and args.memory_entries:
    now = datetime.now()
    at = dict(year=now.year, month=now.month, day=now.day, hour=now.hour, minute=now.minute, weekday=now.weekday())
    entries = [dict(id=i+1, accepts=1, context='', replaced=None, scope='benchmark:contract', kind='confirmed', mode='English', code=f'unused{i}', text=f'vocabulary{i}', at=at) for i in range(args.memory_entries)]
    Path(temporary.name, 'personal_memory.json').write_text(json.dumps(dict(version=1, enabled=True, profile='benchmark:contract', retention_days=30, entries=entries, next_id=args.memory_entries+1)))
command = [str(args.binary.resolve())] + (['--data-dir', temporary.name] if args.persistent else ['--ephemeral'])
with subprocess.Popen(command, stdin=subprocess.PIPE,
                      stdout=subprocess.PIPE, text=True) as process:
    def send(command, timed=True):
        global sequence
        sequence += 1
        start = time.perf_counter()
        process.stdin.write(json.dumps({'version': 1, 'id': sequence, 'command': command}) + '\n')
        process.stdin.flush()
        line = process.stdout.readline()
        if not line:
            raise RuntimeError('backend exited before responding')
        response = json.loads(line)
        if response.get('id') != sequence or not response.get('ok'):
            raise RuntimeError('backend protocol failure')
        if timed:
            samples.append((time.perf_counter() - start) * 1000)
        return response
    send({'type': 'activate', 'context': 'bench', 'mode': 'Mixed', 'app': 'Document'}, False)
    if args.persistent:
        send({'type': 'memory_scope', 'scope': 'benchmark:contract'}, False)
    startup_ms = (time.perf_counter() - started) * 1000
    if args.scenario == 'rerank':
        time.sleep(2)
    for iteration in range(args.iterations):
        if args.scenario == 'context':
            send({'type': 'surrounding', 'context': 'bench', 'before_cursor': 'i want ', 'after_cursor': ''}, False)
        if args.scenario == 'rerank':
            before, after, text = contexts[iteration % len(contexts)]
            send({'type': 'surrounding', 'context': 'bench', 'before_cursor': before,
                  'after_cursor': after}, False)
        else:
            text = 'some' if args.scenario == 'context' else 'su3cl3g4ru,4su3cl3g4ru,4'
        for char in text:
            key = {'name': 'space'} if char == ' ' else {'name': 'character', 'value': char}
            response = send({'type': 'key', 'context': 'bench', 'key': key})
        if args.scenario == 'context':
            if not response['view'].get('suggestion') or response['view']['suggestion'] == ' eats':
                raise RuntimeError('expected document-context completion is missing')
            send({'type': 'key', 'context': 'bench', 'key': {'name': 'tab'}})
        if args.scenario == 'rerank':
            view = response['view']
            draft = view['converted_preedit']
            start = time.perf_counter()
            while view.get('model_pending') and time.perf_counter() - start < .3:
                time.sleep(.02)
                view = send({'type': 'poll_word_model', 'context': 'bench',
                             'revision': view['decode_revision']}, False)['view']
            if draft != view['converted_preedit']:
                model_updates += 1
                settle.append((time.perf_counter() - start) * 1000)
            draft = view['converted_preedit']
            final = send({'type': 'key', 'context': 'bench', 'key': {'name': 'enter'}})
            if final['commit'] != draft:
                raise RuntimeError('reranked preview changed at commit')
            conversions[f'{before}〔{draft}〕{after}'] += 1
        if args.persistent and args.scenario == 'zhuyin':
            send({'type': 'key', 'context': 'bench', 'key': {'name': 'enter'}})
        send({'type': 'reset', 'context': 'bench'})
    if args.persistent:
        send({'type': 'memory_flush'}, False)
    if args.resources:
        # Model workers can be spawned by a host thread other than its leader.
        tasks = Path('/proc', str(process.pid), 'task')
        children = {int(pid) for status in tasks.glob('*/children')
                    for pid in status.read_text().split()}
        resources = [process_memory(process.pid)]
        for pid in sorted(children):
            try:
                resources.append(process_memory(pid))
            except FileNotFoundError:
                # A model worker may have exited since the process listing.
                continue
    process.stdin.close()
    process.wait(timeout=5)
temporary.cleanup()
samples.sort()
print(json.dumps({'persistent': args.persistent, 'seeded_memory_entries': args.memory_entries, 'scenario': args.scenario, 'requests': len(samples), 'startup_ms': round(startup_ms, 3),
                  'median_ms': round(statistics.median(samples), 3),
                  'p95_ms': round(samples[int((len(samples)-1)*.95)], 3),
                  'max_ms': round(max(samples), 3),
                  **({'process_memory': resources} if args.resources else {}),
                  **({'conversions': conversions, 'deferred_changes': model_updates,
                      'median_deferred_ms': round(statistics.median(settle), 3) if settle else None}
                     if args.scenario == 'rerank' else {})}, indent=2,
                 ensure_ascii=False))
