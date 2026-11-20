#!/usr/bin/env python3
"""Build the bundled subset: python scripts/build_zhuyin.py Properties.zip MapingTables.zip.

Archives: https://www.cns11643.gov.tw/opendata/{Properties,MapingTables}.zip
No network access; output is restricted to the existing demo vocabulary.
"""
import hashlib
from pathlib import Path
import re
import sys
import zipfile

ROOT = Path(__file__).resolve().parents[1]
KEYS = '1qaz2wsxedcrfv5tgbyhnujm8ik,9ol.0p;/-'
SYMBOLS = 'ㄅㄆㄇㄈㄉㄊㄋㄌㄍㄎㄏㄐㄑㄒㄓㄔㄕㄖㄗㄘㄙㄧㄨㄩㄚㄛㄜㄝㄞㄟㄠㄡㄢㄣㄤㄥㄦ'
KEY = dict(zip(SYMBOLS, KEYS))
TONE = {'ˊ': '6', 'ˇ': '3', 'ˋ': '4', '˙': '7'}


def raw(reading):
    return ''.join(KEY[c] for c in reading if c in KEY) + next((TONE[c] for c in reading if c in TONE), ' ')


def main():
    props, maps = map(Path, sys.argv[1:])
    pz, mz = zipfile.ZipFile(props), zipfile.ZipFile(maps)
    unicode = {}
    for name in mz.namelist():
        if name.startswith('Unicode/') and name.endswith('.txt'):
            for line in mz.read(name).decode('utf-8-sig').splitlines():
                fields = line.split('\t')
                if len(fields) == 2:
                    unicode[fields[0]] = chr(int(fields[1], 16))
    pinyin = {}
    for line in pz.read('CNS_pinyin_1.txt').decode('utf-8-sig').splitlines():
        fields = line.split('\t')
        if len(fields) >= 2:
            pinyin[fields[0]] = re.sub('[1-5]', '', fields[1]).replace('ü', 'v')
    readings = {}
    for line in pz.read('CNS_phonetic.txt').decode('utf-8-sig').splitlines():
        code, reading = line.split('\t')
        if code in unicode and reading in pinyin:
            item = (pinyin[reading], raw(reading))
            bucket = readings.setdefault(unicode[code], [])
            if item not in bucket:
                bucket.append(item)
    # Reviewed phrase readings override isolated-character ambiguity and neutral tones.
    overrides = {}
    override_path = ROOT / 'crates/senti-predictors/data/zhuyin_overrides.tsv'
    for line in override_path.read_text().splitlines():
        if line and not line.startswith('#'):
            text, reading = line.split('\t')
            overrides[text] = ''.join(raw(s) for s in reading.split())
    out = ['# Dachen keys (space = first tone)\ttext\tfrequency\ttags',
           '# Derived from CNS11643; see SOURCES.md. Do not trim column 1.']
    missing = []
    ambiguous = []
    for filename in ['pinyin_chars.tsv', 'pinyin_words.tsv']:
        for line in (ROOT / 'crates/senti-predictors/data' / filename).read_text().splitlines():
            if not line or line.startswith('#'):
                continue
            fields = line.split('\t')
            py, text, freq = fields[:3]
            tags = fields[3] if len(fields) > 3 else ''
            choices = [[r for p, r in readings.get(ch, []) if p == syllable]
                       for ch, syllable in zip(text, py.split("'"))]
            if text in overrides:
                codes = [overrides[text]]
            elif len(py.split("'")) != len(text) or len(choices) != len(text) or not all(choices):
                missing.append((py, text)); continue
            elif len(text) == 1:
                codes = choices[0]
            else:
                if any(len(c) > 1 for c in choices):
                    ambiguous.append((text, choices))
                codes = [''.join(c[0] for c in choices)]
            for code in codes:
                out.append(f'{code}\t{text}\t{freq}\t{tags}')
    if missing or ambiguous:
        print('Missing:', missing)
        print('Ambiguous phrases requiring override:', ambiguous)
        raise SystemExit('Add reviewed overrides before generating.')
    (ROOT / 'crates/senti-predictors/data/zhuyin.tsv').write_text('\n'.join(out) + '\n')
    print('Generated', len(out) - 2, 'entries')
    for archive in [props, maps]:
        print(archive.name, hashlib.sha256(archive.read_bytes()).hexdigest())


if __name__ == '__main__':
    main()
