#!/usr/bin/env python3
"""Audit anchored candidates and earlier-tone alternatives on held-out data."""
import argparse
import json
from pathlib import Path
import subprocess


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('audit', type=Path)
    parser.add_argument('dataset', type=Path)
    args = parser.parse_args()
    rows = [json.loads(line) for line in args.dataset.read_text().splitlines()]
    samples = [row for row in rows if row['kind'] == 'wrong_tone'
               or (row['kind'] == 'clean' and len(row['text']) >= 4)]
    requests = []
    for row in samples:
        text = row['text']
        start = (len(text) - 2) // 2
        requests.append(json.dumps([row['left'], row['raw'], text[:start], text[start + 2:]]))
    output = subprocess.run([str(args.audit.resolve())], text=True,
                            input=''.join(line + '\n' for line in requests),
                            stdout=subprocess.PIPE, check=True)
    results = [json.loads(line) for line in output.stdout.splitlines()]
    if len(results) != len(samples):
        raise RuntimeError('audit did not return one hypothesis list per row')
    clean = [(row['text'], result) for row, result in zip(samples, results) if row['kind'] == 'clean']
    typo = [(row['text'], result) for row, result in zip(samples, results) if row['kind'] == 'wrong_tone']
    print(json.dumps({
        'clean_cases': len(clean),
        'unanchored_hits': sum(text in result['model'] for text, result in clean),
        'anchored_hits': sum(text in result['anchored'] for text, result in clean),
        'wrong_tone_cases': len(typo),
        'exact_before': sum(text in result['graph'] for text, result in typo),
        'exact_or_earlier_tone': sum(text in result['graph'] or text in result['earlier_tones']
                                     for text, result in typo),
    }, indent=2))


if __name__ == '__main__':
    main()
