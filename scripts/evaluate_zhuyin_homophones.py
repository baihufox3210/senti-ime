#!/usr/bin/env python3
"""Compare fixed dictionary candidates on unambiguous readings in UD GSD test."""
import argparse
from collections import defaultdict
import json
from pathlib import Path
import struct

import numpy as np

from train_neural import ROOT, read_corpus

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('models', type=Path, nargs='+')
args = parser.parse_args()

data = ROOT / 'crates/senti-predictors/data'
readings = defaultdict(set)
groups = defaultdict(dict)
for name in ('zhuyin.tsv', 'zhuyin_extra.tsv'):
    for row in (data / name).read_text().splitlines():
        if not row or row.startswith('#'):
            continue
        code, text, frequency, *_ = row.split('\t')
        if len(text) == 1:
            readings[text].add(code)
            groups[code][text] = max(groups[code].get(text, 0), int(frequency))

vocabularies = [
    json.loads((model / 'vocab.json').read_text())['tokens'] for model in args.models
]
common = set.intersection(*(set(vocab) for vocab in vocabularies))
cases = []
# read_corpus places English first. Chinese targets with dictionary readings
# select the Chinese test positions and keep sentence order intact.
for sentence in read_corpus(ROOT / 'target/training-corpus', 'test'):
    context = []
    for word, _ in sentence:
        if word in common and len(readings[word]) == 1:
            code = next(iter(readings[word]))
            candidates = sorted(groups[code].keys() & common)
            if context and len(candidates) >= 2:
                cases.append((context[-16:], word, candidates, groups[code]))
        context.append(word)

report = {'cases': len(cases), 'models': {}}
for path, vocab in zip(args.models, vocabularies):
    ids = {token: i for i, token in enumerate(vocab)}
    _, d, _, c, t, _ = struct.unpack('<6I', (path / 'model.bin').read_bytes()[8:32])
    with np.load(path / 'training-checkpoint.npz') as model:
        contexts = [[ids.get(token, 1) for token in before] for before, *_ in cases]
        ordered = np.array([([0] * c + before)[-c:] for before in contexts])
        topic = np.array([([0] * t + before)[-t:] for before in contexts])
        embeddings, weights = model['E'], model['topic_weights']
        weights = weights[topic]
        weights /= np.maximum(weights.sum(1, keepdims=True), 1e-6)
        features = np.concatenate([
            embeddings[ordered].reshape(len(cases), c * d),
            (embeddings[topic] * weights[:, :, None]).sum(1),
        ], axis=1)
        hidden = np.tanh(features @ model['W'] + model['b'])
        output, bias = model['O'], model['ob']
        wins = {'frequency': 0, 'neural': 0, 'combined': 0}
        for enc, (_, answer, candidates, frequencies) in zip(hidden, cases):
            indices = [ids[word] for word in candidates]
            prior = np.log([frequencies[word] + 1 for word in candidates])
            logits = output[indices] @ enc + bias[indices]
            for name, score in [('frequency', prior), ('neural', logits), ('combined', prior + logits)]:
                wins[name] += candidates[int(np.argmax(score))] == answer
        report['models'][path.name] = {key: value / len(cases) for key, value in wins.items()}
print(json.dumps(report, indent=2))
