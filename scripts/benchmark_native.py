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

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--binary', type=Path, default=Path('target/release/senti-host'))
parser.add_argument('--iterations', type=int, default=20)
parser.add_argument('--scenario', choices=['zhuyin', 'context'], default='zhuyin')
parser.add_argument('--persistent', action='store_true', help='use isolated real disk storage and include commits')
parser.add_argument('--memory-entries', type=int, default=0, help='seed up to 512 synthetic vocabulary records (requires --persistent)')
args = parser.parse_args()
if not 0 <= args.memory_entries <= 512 or args.memory_entries and not args.persistent:
    parser.error('--memory-entries must be 0–512 and requires --persistent')
if not 1 <= args.iterations <= 1000:
    parser.error('--iterations must be between 1 and 1000')
samples = []
sequence = 0
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
    for _ in range(args.iterations):
        if args.scenario == 'context':
            send({'type': 'surrounding', 'context': 'bench', 'before_cursor': 'i want ', 'after_cursor': ''}, False)
        text = 'some' if args.scenario == 'context' else 'su3cl3g4ru,4su3cl3g4ru,4'
        for char in text:
            key = {'name': 'space'} if char == ' ' else {'name': 'character', 'value': char}
            response = send({'type': 'key', 'context': 'bench', 'key': key})
        if args.scenario == 'context':
            if not response['view'].get('suggestion') or response['view']['suggestion'] == ' eats':
                raise RuntimeError('expected document-context completion is missing')
            send({'type': 'key', 'context': 'bench', 'key': {'name': 'tab'}})
        if args.persistent and args.scenario == 'zhuyin':
            send({'type': 'key', 'context': 'bench', 'key': {'name': 'enter'}})
        send({'type': 'reset', 'context': 'bench'})
    if args.persistent:
        send({'type': 'memory_flush'}, False)
    process.stdin.close()
    process.wait(timeout=5)
temporary.cleanup()
samples.sort()
print(json.dumps({'persistent': args.persistent, 'seeded_memory_entries': args.memory_entries, 'scenario': args.scenario, 'requests': len(samples), 'startup_ms': round(startup_ms, 3),
                  'median_ms': round(statistics.median(samples), 3),
                  'p95_ms': round(samples[int((len(samples)-1)*.95)], 3),
                  'max_ms': round(max(samples), 3)}, indent=2))
