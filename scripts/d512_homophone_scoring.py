"""CPU scoring for the official 0c D512 ONNX model (MIT).
See https://huggingface.co/Funmula/0c-sequence-d512 for the input format.
"""
import json
import sys
from pathlib import Path

import numpy as np
import onnxruntime as ort


class D512:
    def __init__(self, path):
        vocab = json.loads((path / 'vocab.json').read_text())
        self.chars = {char: index for index, char in enumerate(vocab['characters'], start=8)}
        self.bases = {base: index for index, base in enumerate(vocab['bases'], start=2)}
        options = ort.SessionOptions()
        options.intra_op_num_threads = 2
        self.session = ort.InferenceSession(
            str(path / 'model.onnx'), sess_options=options, providers=['CPUExecutionProvider'],
        )

    def supports(self, sounds):
        return all(base in self.bases for base, _ in sounds)

    def scores(self, context, sounds, prefix, alternatives):
        # All options share the known prefix, so each target row reads the
        # same projected nodes. Only the final target differs.
        head = [1, *(self.chars.get(c, 4) for c in context[-64:]), 2]
        prefix_ids = [self.chars[c] for c in prefix]
        targets = np.array(
            [prefix_ids + [self.chars[c]] for c in alternatives], dtype=np.int64,
        )
        count = targets.shape[1]
        offset = len(head) + count
        length = offset + count
        tone_ids = {'': 1, 'ˊ': 2, 'ˇ': 3, 'ˋ': 4, '˙': 5}

        def row(values):
            return np.array([values], dtype=np.int64)

        inputs = {
            'ids': row(head + [0] * count + [3] + prefix_ids),
            'bases': row([0] * len(head) + [self.bases[b] for b, _ in sounds] + [0] * count),
            'tones': row([0] * len(head) + [tone_ids[t] for _, t in sounds] + [0] * count),
            'types': row([0] * len(head) + [1] * count + [0] + [2] * (count - 1)),
            'positions': row(list(range(length))),
            'mask': np.tri(length, dtype=bool)[None, None],
            'proj_pos': np.arange(offset, length, dtype=np.int64),
            'target_node': np.tile(np.arange(count, dtype=np.int64), (len(alternatives), 1)),
            'target_ids': targets - 6,
        }
        return self.session.run(['scores'], inputs)[0]


def rank_options(model, previous, sounds, candidates):
    if not model.supports(sounds) or any(c not in model.chars for word in candidates for c in word):
        return None
    # One causal tree shares the reading/context and each candidate prefix.
    n = len(sounds)
    head = [1, *(model.chars.get(c, 4) for c in previous[-64:]), 2]
    offset = len(head) + n
    prefixes = list(dict.fromkeys(word[:i] for word in candidates for i in range(n)))
    nodes = {prefix: i for i, prefix in enumerate(prefixes)}
    ids = head + [0] * n + [3 if not p else model.chars[p[-1]] for p in prefixes]
    bases = [0] * len(head) + [model.bases[base] for base, _ in sounds] + [0] * len(prefixes)
    tone_ids = {'': 1, 'ˊ': 2, 'ˇ': 3, 'ˋ': 4, '˙': 5}
    tones = [0] * len(head) + [tone_ids[tone] for _, tone in sounds] + [0] * len(prefixes)
    types = [0] * len(head) + [1] * n + [0 if not p else 2 for p in prefixes]
    mask = np.tri(len(ids), dtype=bool)
    for i, prefix in enumerate(prefixes):
        mask[offset + i, offset:] = False
        for j in range(len(prefix) + 1):
            mask[offset + i, offset + nodes[prefix[:j]]] = True

    def row(values):
        return np.array([values], dtype=np.int64)

    inputs = {
        'ids': row(ids), 'bases': row(bases), 'tones': row(tones), 'types': row(types),
        'positions': row(list(range(offset)) + [offset + len(prefix) for prefix in prefixes]),
        'mask': mask[None, None],
        'proj_pos': np.arange(offset, len(ids), dtype=np.int64),
        'target_node': np.array([[nodes[word[:i]] for i in range(n)] for word in candidates], dtype=np.int64),
        'target_ids': np.array([[model.chars[c] - 6 for c in word] for word in candidates], dtype=np.int64),
    }
    scores = model.session.run(['scores'], inputs)[0]
    return scores.tolist() if np.isfinite(scores).all() else None


def serve():
    model = D512(Path(sys.argv[1]))
    print('ready', flush=True)
    for line in sys.stdin:
        try:
            previous, sounds, candidates = json.loads(line)
            scores = rank_options(model, previous, sounds, candidates)
            print(json.dumps(scores), flush=True)
        except (ValueError, KeyError, RuntimeError):
            print('null', flush=True)


if __name__ == '__main__':
    serve()
