#!/usr/bin/env python3
"""Train a same-reading span ranker using intact Chinese-GSD training text."""
import argparse
import hashlib
import json
from pathlib import Path
import struct

import numpy as np

from finetune_zhuyin_head import candidates
from train_neural import ROOT, read_corpus
from train_zhuyin_ngrams import is_han


def pairs(corpus, split, ids, options, seed):
    rng = np.random.default_rng(seed)
    for sentence in read_corpus(corpus, split):
        words = [word for word, _ in sentence]
        encoded = [ids.get(word, 1) for word in words]
        for start in range(len(words) - 1):
            for width in range(2, min(4, len(words) - start) + 1):
                span = words[start:start + width]
                if not all(len(word) == 1 and is_han(word) for word in span):
                    continue
                correct = encoded[start:start + width]
                if 1 in correct:
                    continue
                positions = [i for i, token in enumerate(correct) if token in options]
                if not positions:
                    continue
                position = int(rng.choice(positions))
                yield encoded[max(0, start - 16):start], correct, position, options[correct[position]][1:5]


def features(records, state, dims):
    _, d, h, context, topic, _ = dims
    histories, targets, segments = [], [], []
    for row, (prefix, correct, alternative) in enumerate(records):
        for side, word in enumerate((correct, alternative)):
            for index, token in enumerate(word):
                history = [0] * topic + prefix + word[:index]
                histories.append(history[-topic:])
                targets.append(token)
                segments.append(row * 2 + side)
    histories = np.asarray(histories, dtype=np.int32)
    encoded = np.empty((len(histories), h), dtype=np.float32)
    for start in range(0, len(histories), 2048):
        part = slice(start, start + 2048)
        tokens = histories[part]
        weights = state['topic_weights'][tokens]
        weights /= np.maximum(weights.sum(axis=1, keepdims=True), 1e-6)
        embeddings = state['E'][tokens]
        bag = (embeddings * weights[:, :, None]).sum(axis=1)
        short = embeddings[:, -context:].reshape(-1, context * d)
        encoded[part] = np.tanh(np.concatenate((short, bag), axis=1) @ state['W'] + state['b'])
    return encoded, np.asarray(targets), np.asarray(segments)


def mine(records, state, dims):
    """Keep the strongest wrong span under the starting checkpoint."""
    for start in range(0, len(records), 256):
        expanded, references = [], []
        for prefix, correct, position, challengers in records[start:start + 256]:
            references.append(len(expanded))
            for token in challengers:
                alternative = correct.copy()
                alternative[position] = token
                expanded.append((prefix, correct, alternative))
        encoded, targets, segments = features(expanded, state, dims)
        scores = np.einsum('bh,bh->b', encoded, state['O'][targets]) + state['ob'][targets]
        means = np.bincount(segments, weights=scores) / np.bincount(segments)
        for begin, end in zip(references, references[1:] + [len(expanded)]):
            yield expanded[begin + np.argmax(means[1::2][begin:end])]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--base', required=True, type=Path)
    parser.add_argument('--output', required=True, type=Path)
    parser.add_argument('--corpus', type=Path, default=ROOT / 'target/training-corpus')
    parser.add_argument('--rate', type=float, default=.035)
    parser.add_argument('--epochs', type=int, default=4)
    args = parser.parse_args()
    if args.epochs < 1 or args.rate <= 0:
        parser.error('epochs and rate must be positive')

    raw = (args.base / 'model.bin').read_bytes()
    vocabulary = (args.base / 'vocab.json').read_bytes()
    tokens = json.loads(vocabulary)['tokens']
    dims = struct.unpack('<6I', raw[8:32])
    with np.load(args.base / 'training-checkpoint.npz') as checkpoint:
        state = dict(checkpoint)
    ids, options = candidates(tokens)
    rng = np.random.default_rng(6128)
    train = list(pairs(args.corpus, 'train', ids, options, 6128))
    # Use a fixed subsample so the training memory stays bounded.
    train = [train[i] for i in rng.choice(len(train), min(32_000, len(train)), replace=False)]
    validation = list(pairs(args.corpus, 'dev', ids, options, 7119))
    validation = validation[::4]
    train = list(mine(train, state, dims))
    pairs_sha256 = hashlib.sha256(json.dumps(train, separators=(',', ':')).encode()).hexdigest()
    dev = features(list(mine(validation, state, dims)), state, dims)
    output, bias = state['O'], state['ob']
    old_output, old_bias = output.copy(), bias.copy()

    def margins(batch):
        encoded, targets, segments = batch
        scores = np.einsum('bh,bh->b', encoded, output[targets]) + bias[targets]
        means = np.bincount(segments, weights=scores) / np.bincount(segments)
        return means[::2] - means[1::2]

    def measure():
        scores = margins(dev)
        return {'wins': int((scores > 0).sum()), 'cases': len(scores),
                'nll': float(np.logaddexp(0, -scores).mean())}

    before = best = measure()
    best_parameters = old_output, old_bias
    history = []
    for epoch in range(args.epochs):
        order = rng.permutation(len(train))
        for start in range(0, len(order), 128):
            batch = features([train[i] for i in order[start:start + 128]], state, dims)
            encoded, targets, segments = batch
            scores = margins(batch)
            probability = 1 / (1 + np.exp(scores.clip(-20, 20)))
            sign = 1 - 2 * (segments % 2)
            step = args.rate / len(scores)
            delta = step * sign * probability[segments // 2] / np.bincount(segments)[segments]
            np.add.at(output, targets, delta[:, None] * encoded)
            np.add.at(bias, targets, delta)
        output[:] = old_output + .96 * (output - old_output)
        bias[:] = old_bias + .96 * (bias - old_bias)
        metrics = measure()
        history.append(metrics)
        print(json.dumps({'epoch': epoch + 1, **metrics}), flush=True)
        if (metrics['wins'], -metrics['nll']) > (best['wins'], -best['nll']):
            best, best_parameters = metrics, (output.copy(), bias.copy())
    output[:], bias[:] = best_parameters
    args.output.mkdir(parents=True, exist_ok=True)
    arrays = [state[key] for key in ('E', 'W', 'b', 'O', 'ob', 'P', 'pb', 'lexpos', 'topic_weights')]
    binary = raw[:32] + b''.join(array.astype('<f4').tobytes() for array in arrays)
    (args.output / 'model.bin').write_bytes(binary)
    (args.output / 'vocab.json').write_bytes(vocabulary)
    np.savez(args.output / 'training-checkpoint.npz', **state)
    report = {'training_pairs': len(train), 'epochs': args.epochs, 'rate': args.rate,
              'base_sha256': hashlib.sha256(raw).hexdigest(),
              'model_sha256': hashlib.sha256(binary).hexdigest(),
              'pairs_sha256': pairs_sha256,
              'dev_before': before, 'dev_after': best, 'history': history}
    (args.output / 'training-report.json').write_text(json.dumps(report, indent=2) + '\n')
    print(json.dumps(report), flush=True)


if __name__ == '__main__':
    main()
