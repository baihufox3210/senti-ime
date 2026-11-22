#!/usr/bin/env python3
"""Fine-tune the Chinese output head on actual same-tone candidates."""
import argparse
from collections import defaultdict
import hashlib
import json
from pathlib import Path
import struct

import numpy as np

from train_neural import ROOT, read_corpus, softmax


def candidates(tokens):
    ids = {word: index for index, word in enumerate(tokens)}
    readings = defaultdict(set)
    groups = defaultdict(dict)
    for name in ('zhuyin.tsv', 'zhuyin_extra.tsv'):
        for row in (ROOT / 'crates/senti-predictors/data' / name).read_text().splitlines():
            if row and not row.startswith('#'):
                code, word, frequency, *_ = row.split('\t')
                if len(word) == 1 and word in ids:
                    index = ids[word]
                    readings[index].add(code)
                    groups[code][index] = max(int(frequency), groups[code].get(index, 0))
    result = {}
    for index, sounds in readings.items():
        if len(sounds) != 1:
            continue
        group = groups[next(iter(sounds))]
        ordered = sorted(group, key=lambda other: (-group[other], other))[:16]
        if index in ordered and len(ordered) > 1:
            result[index] = [index] + [other for other in ordered if other != index]
    return ids, result


def examples(corpus, split, ids, options, state, dims):
    _, d, _, context, topic, _ = dims
    short, long, choices = [], [], []
    for sentence in read_corpus(corpus, split):
        history = [0] * topic
        for word, _ in sentence:
            index = ids.get(word, 1)
            if index in options:
                short.append(history[-context:])
                long.append(history[-topic:])
                choices.append(options[index] + [0] * (16 - len(options[index])))
            history.append(index)
    short, long, choices = map(np.asarray, (short, long, choices))
    features = []
    for start in range(0, len(choices), 2048):
        part = slice(start, start + 2048)
        weight = state['topic_weights'][long[part]]
        weight /= np.maximum(weight.sum(axis=1, keepdims=True), 1e-6)
        embedding = state['E']
        bag = (embedding[long[part]] * weight[:, :, None]).sum(axis=1)
        flattened = embedding[short[part]].reshape(-1, context * d)
        features.append(np.tanh(np.concatenate((flattened, bag), axis=1) @ state['W'] + state['b']))
    return np.concatenate(features), choices


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--base', type=Path, default=ROOT / 'models/tiny-ime-v2')
    parser.add_argument('--corpus', type=Path, default=ROOT / 'target/training-corpus')
    parser.add_argument('--output', required=True, type=Path)
    parser.add_argument('--epochs', type=int, default=4)
    parser.add_argument('--rate', type=float, default=.10)
    args = parser.parse_args()
    if args.epochs < 1 or args.rate <= 0:
        parser.error('epochs and rate must be positive')

    raw = (args.base / 'model.bin').read_bytes()
    vocabulary = (args.base / 'vocab.json').read_bytes()
    dims = struct.unpack('<6I', raw[8:32])
    with np.load(args.base / 'training-checkpoint.npz') as checkpoint:
        state = dict(checkpoint)
    ids, options = candidates(json.loads(vocabulary)['tokens'])
    x, choices = examples(args.corpus, 'train', ids, options, state, dims)
    dev = examples(args.corpus, 'dev', ids, options, state, dims)
    base_o, base_b = state['O'].copy(), state['ob'].copy()
    output, bias = state['O'], state['ob']

    def logits(features, ids):
        values = np.einsum('bh,bkh->bk', features, output[ids]) + bias[ids]
        values[ids == 0] = -1e9
        return values

    def measure(split):
        hidden, ids = split
        predicted = []
        for start in range(0, len(ids), 1024):
            part = slice(start, start + 1024)
            predicted.append(logits(hidden[part], ids[part]))
        values = np.concatenate(predicted)
        return {'exact': int((values.argmax(axis=1) == 0).sum()), 'cases': len(values),
                'nll': float(np.mean(np.logaddexp.reduce(values, axis=1) - values[:, 0]))}

    before = measure(dev)
    best = before
    best_parameters = (base_o, base_b)
    rng = np.random.default_rng(6083)
    history = []
    for epoch in range(args.epochs):
        order = rng.permutation(len(choices))
        for start in range(0, len(order), 128):
            rows = order[start:start + 128]
            hidden, ids = x[rows], choices[rows]
            error = softmax(logits(hidden, ids))
            error[:, 0] -= 1.0
            step = -args.rate / len(rows)
            np.add.at(output, ids, step * error[:, :, None] * hidden[:, None, :])
            np.add.at(bias, ids, step * error)
        # Shrink the learned residual towards the pretrained head.
        output[:] = base_o + .97 * (output - base_o)
        bias[:] = base_b + .97 * (bias - base_b)
        metrics = measure(dev)
        history.append(metrics)
        print(json.dumps({'epoch': epoch + 1, **metrics}), flush=True)
        if (metrics['exact'], -metrics['nll']) > (best['exact'], -best['nll']):
            best = metrics
            best_parameters = (output.copy(), bias.copy())
    output[:], bias[:] = best_parameters
    args.output.mkdir(parents=True, exist_ok=True)
    arrays = [state[key] for key in ('E', 'W', 'b', 'O', 'ob', 'P', 'pb', 'lexpos', 'topic_weights')]
    binary = raw[:32] + b''.join(array.astype('<f4').tobytes() for array in arrays)
    (args.output / 'model.bin').write_bytes(binary)
    (args.output / 'vocab.json').write_bytes(vocabulary)
    np.savez(args.output / 'training-checkpoint.npz', **state)
    report = {'base_sha256': hashlib.sha256(raw).hexdigest(),
              'model_sha256': hashlib.sha256(binary).hexdigest(), 'training_cases': len(x),
              'epochs': args.epochs, 'rate': args.rate, 'dev_before': before, 'dev_after': best,
              'corpus_sha256': {split: hashlib.sha256(
                  (args.corpus / f'UD_Chinese-GSD-zh_gsd-ud-{split}.conllu').read_bytes()
              ).hexdigest() for split in ('train', 'dev')},
              'history': history}
    (args.output / 'training-report.json').write_text(json.dumps(report, indent=2) + '\n')
    print(json.dumps(report), flush=True)


if __name__ == '__main__':
    main()
