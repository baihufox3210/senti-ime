#!/usr/bin/env python3
"""Compare fixed dictionary candidates on unambiguous readings in UD GSD test."""
import argparse
from collections import defaultdict
import json
import random
import subprocess
from pathlib import Path
import struct
from time import perf_counter

import numpy as np

from build_zhuyin import KEYS, SYMBOLS, TONE
from train_neural import ROOT, read_corpus

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('models', type=Path, nargs='+')
parser.add_argument('--d512', type=Path, help='Local directory with official 0c ONNX and vocabulary')
parser.add_argument('--qwen', type=Path, help='Prepared directory with senti-qwen, model.gguf and lib/')
parser.add_argument('--sample-size', type=int, default=256, help='Maximum common cases with --d512')
parser.add_argument('--calibration-size', type=int, default=512, help='UD GSD train cases to fit the 0c frequency weight')
args = parser.parse_args()
if args.qwen and not args.d512:
    parser.error('--qwen requires --d512 for the common test cohort')
if args.calibration_size < 1:
    parser.error('--calibration-size must be positive')
if args.sample_size < 1:
    parser.error('--sample-size must be positive')

if args.d512:
    from d512_homophone_scoring import D512
    d512 = D512(args.d512)
    key_to_symbol = dict(zip(KEYS, SYMBOLS))
    key_to_tone = {key: tone for tone, key in TONE.items()}

    def reading(code):
        return ''.join(key_to_symbol[key] for key in code[:-1]), key_to_tone.get(code[-1], '')


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
def evaluation_cases(split):
    cases = []
    d512_cases = []
    # read_corpus places English first. Chinese targets with dictionary readings
    # select the Chinese test positions and keep sentence order intact.
    for sentence in read_corpus(ROOT / 'target/training-corpus', split):
        context = []
        for word, _ in sentence:
            if word in common and len(readings[word]) == 1:
                code = next(iter(readings[word]))
                candidates = sorted(groups[code].keys() & common)
                if context and len(candidates) >= 2:
                    if args.d512:
                        previous = context[-1]
                        if previous not in d512.chars or len(readings[previous]) != 1:
                            continue
                        sounds = [reading(next(iter(readings[previous]))), reading(code)]
                        candidates = sorted(
                            (c for c in candidates if c in d512.chars),
                            key=lambda c: (-groups[code][c], c),
                        )[:6 if args.qwen else 16]
                        if word not in candidates or len(candidates) < 2 or not d512.supports(sounds):
                            continue
                        d512_cases.append((''.join(context[:-1]), sounds, previous))
                    cases.append((context[-16:], word, candidates, groups[code]))
            context.append(word)

    return cases, d512_cases


cases, d512_cases = evaluation_cases('test')
if args.d512:
    common_count = len(cases)
    if not common_count:
        parser.error('no cases share the supplied model vocabularies and readings')
    indices = np.linspace(0, common_count - 1, min(common_count, args.sample_size), dtype=int)
    cases = [cases[i] for i in indices]
    d512_cases = [d512_cases[i] for i in indices]

report = {'cases': len(cases), 'models': {}}
if args.d512:
    report['eligible_common_cases'] = common_count
    train, train_readings = evaluation_cases('train')
    weights = np.linspace(0, 1, 21)
    losses = np.zeros(len(weights))

    def d512_scores(case, details):
        _, answer, candidates, frequencies = case
        before, sounds, previous = details
        scores = d512.scores(before, sounds, previous, candidates)
        prior = np.log1p([frequencies[c] for c in candidates])
        return candidates.index(answer), scores, prior

    # Train one nonnegative dictionary-prior coefficient by minimizing ranking
    # cross-entropy on a deterministic sample from the separate train split.
    indices = np.linspace(0, len(train) - 1, min(len(train), args.calibration_size), dtype=int)
    for i in indices:
        target, scores, prior = d512_scores(train[i], train_readings[i])
        fused = scores + weights[:, None] * prior
        losses += np.logaddexp.reduce(fused, axis=1) - fused[:, target]
    weight = float(weights[np.argmin(losses)])

    # Verify that sharing the prefix preserves independent per-option scores.
    before, sounds, previous = d512_cases[0]
    candidates = cases[0][2]
    batched = d512.scores(before, sounds, previous, candidates)
    serial = np.concatenate([d512.scores(before, sounds, previous, [c]) for c in candidates])
    np.testing.assert_allclose(batched, serial, atol=1e-4, rtol=1e-4)
    neural_wins = combined_wins = 0
    timings = []
    for case, details in zip(cases, d512_cases):
        start = perf_counter()
        target, scores, prior = d512_scores(case, details)
        timings.append((perf_counter() - start) * 1000)
        neural_wins += int(np.argmax(scores)) == target
        combined_wins += int(np.argmax(scores + weight * prior)) == target
    report['models']['0c-D512'] = {
        'neural': neural_wins / len(cases),
        'combined': combined_wins / len(cases),
        'frequency_weight': weight,
        'calibration_examples': len(indices),
        'batched_scoring_ms_p50': float(np.median(timings)),
        'batched_scoring_ms_p95': float(np.percentile(timings, 95)),
    }
if args.qwen:
    binary = args.qwen.resolve()
    command = [binary / 'senti-qwen', binary / 'model.gguf', binary / 'lib']
    wins, timings = 0, []
    rng = random.Random(42)
    with subprocess.Popen(command, stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                          stderr=subprocess.DEVNULL, text=True) as worker:
        if worker.stdout.readline().strip() != 'ready':
            raise RuntimeError('Qwen worker could not open the model')
        for (_, answer, candidates, _), (before, _, previous) in zip(cases, d512_cases):
            options = rng.sample(candidates, len(candidates))
            payload = (before[-64:], '', [previous + option for option in options])
            start = perf_counter()
            worker.stdin.write(json.dumps(payload, ensure_ascii=False) + '\n')
            worker.stdin.flush()
            scores = json.loads(worker.stdout.readline())
            timings.append((perf_counter() - start) * 1000)
            if scores is None or len(scores) != len(options):
                raise RuntimeError('Qwen returned an invalid score vector')
            wins += options[int(np.argmax(scores))] == answer
        worker.stdin.close()
    report['models']['Qwen3-0.6B-Q8_0'] = {
        'neural': wins / len(cases),
        'scoring_ms_p50': float(np.median(timings)),
        'scoring_ms_p95': float(np.percentile(timings, 95)),
    }

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
