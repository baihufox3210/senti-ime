"""CPU scoring for the official 0c D512 ONNX model (MIT).
See https://huggingface.co/Funmula/0c-sequence-d512 for the input format.
"""
import json

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
