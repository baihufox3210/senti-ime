#!/usr/bin/env python3
"""Build offline lexicon extensions, or import a pinned upstream dictionary subset.

Rebuild from bundled subset:
    python3 scripts/import_mcbopomofo.py
Import upstream files downloaded from the revision below:
    python3 scripts/import_mcbopomofo.py --source-dir DIR --properties Properties.zip
No downloads, runtime dependencies, or modification of the original seed dictionaries.
"""
import argparse
from collections import defaultdict
import hashlib
import json
from pathlib import Path
import re
import zipfile

from build_zhuyin import KEYS, SYMBOLS

ROOT = Path(__file__).resolve().parents[1]
DATA = ROOT / 'crates/senti-predictors/data'
REVISION = '1b7fbb999a354ba80e5cf20820ddd4093c6ed77e'
UPSTREAM = f'https://github.com/openvanilla/McBopomofo/tree/{REVISION}'
SOURCE_HASHES = {
    'BPMFMappings.txt': '5d6accefba88b57a505eb25326b5d813d4e529c698707883c664a6a3fa58b443',
    'BPMFBase.txt': '46e64a53dc9c6baa2cbe6e0b1af572f8ef6aafb50597178b5fd53df804316ce7',
    'phrase.occ': 'a64d57e38b24f1dcc5c1ce31cc3b2244f90206033936c1dc6ca9c6e54a552e8d',
}
PROPERTIES_HASH = '3d56ef14cc8099893245dac58fe4718d2fa64812b9159352a98a4588ad3efa5c'
TARGET_PHRASES = 80000
KEY = dict(zip(SYMBOLS, KEYS))
TONES = {'ˊ': '6', 'ˇ': '3', 'ˋ': '4', '˙': '7'}
NUMERALS = set('〇零一二三四五六七八九十百千萬億兆兩壹貳參肆伍陸柒捌玖拾佰仟')
SUBSET = DATA / 'mcbopomofo_subset.tsv'
DOMAIN = DATA / 'domain_terms.tsv'


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def syllable_code(reading):
    """Validate source spelling and preserve first-tone spaces in the output."""
    marks = [c for c in reading if c in TONES]
    body = ''.join(c for c in reading if c not in TONES)
    if not body or len(marks) > 1 or any(c not in KEY for c in body):
        raise ValueError(f'Unsupported Zhuyin syllable: {reading!r}')
    if marks and not (reading.endswith(marks[0]) or marks[0] == '˙' and reading.startswith('˙')):
        raise ValueError(f'Misplaced tone mark: {reading!r}')
    classes = [1 if c in 'ㄧㄨㄩ' else 2 if c in 'ㄚㄛㄜㄝㄞㄟㄠㄡㄢㄣㄤㄥㄦ' else 0 for c in body]
    if any(a >= b for a, b in zip(classes, classes[1:])):
        raise ValueError(f'Invalid syllable order: {reading!r}')
    return ''.join(KEY[c] for c in body) + (TONES[marks[0]] if marks else ' ')


def seed_records():
    return [line.split('\t') for line in (DATA / 'zhuyin.tsv').read_text().splitlines()
            if line and not line.startswith('#')]


def normalise(count, maximum, ceiling):
    # Preserve upstream frequency ratios on a scale compatible with the seed data.
    return max(1, (count * ceiling + maximum // 2) // maximum)


def import_source(directory, properties):
    for name, expected in SOURCE_HASHES.items():
        if digest(directory / name) != expected:
            raise ValueError(f'{name}: expected pinned upstream SHA-256 {expected}')
    if digest(properties) != PROPERTIES_HASH:
        raise ValueError('Properties.zip does not match the documented CNS11643 snapshot')
    # Use the same Hanyu pinyin convention as the existing dictionary.
    table_source = (ROOT / 'crates/senti-predictors/src/pinyin/syllables.rs').read_text().split('];', 1)[0]
    valid_pinyin = {initial + final for initial, finals in re.findall(r'\(\s*"([a-z]*)",\s*"([a-z ]+)"', table_source)
                    for final in finals.split()}
    with zipfile.ZipFile(properties) as archive:
        phonetic_to_pinyin = {}
        for line in archive.read('CNS_pinyin_1.txt').decode('utf-8-sig').splitlines():
            reading, pinyin, *_ = line.split('\t')
            pinyin = re.sub('[1-5]', '', pinyin).replace('ü', 'v')
            if pinyin in valid_pinyin:
                phonetic_to_pinyin[syllable_code(reading)] = pinyin

    counts = {}
    for line in (directory / 'phrase.occ').read_text().splitlines():
        text, count = line.split()
        counts[text] = int(count)
    seeds = seed_records()
    seed_texts = {fields[1] for fields in seeds}
    seed_pairs = {(fields[0], fields[1]) for fields in seeds}
    seed_phrases = {text for text in seed_texts if len(text) > 1}
    options = defaultdict(set)
    for line in (directory / 'BPMFMappings.txt').read_text().splitlines():
        text, *readings = line.split()
        if not 2 <= len(text) <= 6 or len(text) != len(readings) or counts.get(text, 0) <= 0:
            continue
        if text in seed_texts or set(text) <= NUMERALS or not all('\u4e00' <= ch <= '\u9fff' for ch in text):
            continue
        try:
            codes = [syllable_code(s) for s in readings]
            pinyin = "'".join(phonetic_to_pinyin[s] for s in codes)
        except (UnicodeEncodeError, ValueError, KeyError):
            continue
        options[text].add((' '.join(readings), pinyin))
    chosen = sorted(options, key=lambda text: (-counts[text], text))[:max(0, TARGET_PHRASES - len(seed_phrases))]
    if len(chosen) + len(seed_phrases) < TARGET_PHRASES:
        raise ValueError('Insufficient eligible distinct phrases')
    max_phrase_count = max((counts[text] for text in chosen), default=1)
    rows = []
    required_characters = set()
    for text in chosen:
        for readings, pinyin in sorted(options[text]):
            rows.append((text, readings, pinyin, counts[text], normalise(counts[text], max_phrase_count, 3000)))
            for ch, reading in zip(text, readings.split()):
                if (syllable_code(reading), ch) not in seed_pairs:
                    required_characters.add((ch, reading))
    base = {(fields[0], fields[1]) for line in (directory / 'BPMFBase.txt').read_text().splitlines()
            if len(fields := line.split()) >= 2 and len(fields[0]) == 1
            and ('\u3400' <= fields[0] <= '\u9fff' or '\U00020000' <= fields[0] <= '\U000323af')}
    # Include every supported standalone Han character, not only characters
    # that happen to appear in the top phrase subset.
    for ch, reading in base:
        try:
            code = syllable_code(reading)
        except ValueError:
            continue
        if code in phonetic_to_pinyin and (code, ch) not in seed_pairs:
            required_characters.add((ch, reading))
    base_characters = {ch for ch, _ in base}
    max_char_count = max((counts.get(ch, 0) for ch, _ in required_characters), default=1)
    for ch, reading in sorted(required_characters):
        # Word-specific neutral tones (e.g. 哥哥) can be absent from the
        # isolated-character base. Preserve the attested phrase pronunciation.
        if ch not in base_characters:
            raise ValueError(f'Phrase character missing in BPMFBase.txt: {ch}')
        code = syllable_code(reading)
        rows.append((ch, reading, phonetic_to_pinyin[code], counts.get(ch, 0), normalise(counts.get(ch, 0), max_char_count, 2000)))
    # Retain a small, auditable source subset so ordinary rebuilds need no upstream download.
    content = '# McBopomofo subset; see SOURCES.md and licenses/.\n# text\tZhuyin syllables\tpinyin\tupstream count\tSentiIME frequency\n'
    content += ''.join('\t'.join(map(str, row)) + '\n' for row in sorted(set(rows)))
    SUBSET.write_text(content)


def write_output(path, content, check):
    if check:
        if not path.exists() or path.read_text() != content:
            raise ValueError(f'{path.name} is stale; run scripts/import_mcbopomofo.py')
    else:
        path.write_text(content)


def rebuild(check=False):
    zhuyin = {}
    pinyin = {}
    seed_pairs = {(fields[0], fields[1]) for fields in seed_records()}
    source_rows = ((source, number, line) for source in [SUBSET, DOMAIN]
                   for number, line in enumerate(source.read_text().splitlines(), 1))
    for source, number, line in source_rows:
        if not line or line.startswith('#'):
            continue
        fields = line.split('\t')
        if len(fields) != 5:
            raise ValueError(f'{source.name}:{number}: expected five columns')
        text, reading, py, count, frequency = fields
        syllables = reading.split()
        if len(text) != len(syllables) or len(text) != len(py.split("'")):
            raise ValueError(f'{source.name}:{number}: character/syllable mismatch')
        code = ''.join(syllable_code(s) for s in syllables)
        frequency, count = int(frequency), int(count)
        if frequency <= 0 or count < 0 or not all(ch in "abcdefghijklmnopqrstuvwxyz'" for ch in py):
            raise ValueError(f'{source.name}:{number}: invalid frequency or pinyin')
        if (code, text) in seed_pairs:
            if source == DOMAIN:
                continue  # Seed dictionary retains its original tags/frequency.
            raise ValueError(f'{source.name}:{number}: duplicates a seed reading')
        zhuyin[(code, text)] = max(frequency, zhuyin.get((code, text), 0))
        pinyin[(py, text)] = max(frequency, pinyin.get((py, text), 0))
    header = '# Generated by scripts/import_mcbopomofo.py; see SOURCES.md and licenses/.\n'
    outputs = {}
    for name, records in [('zhuyin_extra.tsv', zhuyin), ('pinyin_extra.tsv', pinyin)]:
        outputs[name] = header + ''.join(f'{code}\t{text}\t{freq}\n' for (code, text), freq in sorted(records.items()))
    texts = {fields[1] for fields in seed_records()} | {text for _, text in zhuyin}
    phrases = {text for text in texts if len(text) > 1}
    if len(phrases) < TARGET_PHRASES:
        raise ValueError(f'Expected at least {TARGET_PHRASES} distinct multi-character phrases, got {len(phrases)}')
    manifest = {
        'upstream': UPSTREAM, 'upstream_sha256': SOURCE_HASHES,
        'cns_properties_sha256': PROPERTIES_HASH,
        'subset_sha256': digest(SUBSET), 'domain_sha256': digest(DOMAIN), 'seed_zhuyin_sha256': digest(DATA / 'zhuyin.tsv'),
        'counts': {'seed_readings': len(seed_records()), 'imported_zhuyin_readings': len(zhuyin),
                   'total_zhuyin_readings': len(seed_records()) + len(zhuyin), 'imported_pinyin_readings': len(pinyin),
                   'unique_texts': len(texts), 'unique_phrases': len(phrases), 'unique_characters': len(texts) - len(phrases)},
        'normalisation': {'phrase_ceiling': 3000, 'character_ceiling': 2000, 'minimum': 1},
        'outputs_sha256': {name: hashlib.sha256(content.encode()).hexdigest() for name, content in outputs.items()},
    }
    outputs['lexicon_manifest.json'] = json.dumps(manifest, ensure_ascii=False, indent=2) + '\n'
    for name, content in outputs.items():
        write_output(DATA / name, content, check)
    print(json.dumps(manifest['counts'], ensure_ascii=False))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--source-dir', type=Path)
    parser.add_argument('--properties', type=Path)
    parser.add_argument('--check', action='store_true', help='verify bundled outputs without writing files')
    args = parser.parse_args()
    if bool(args.source_dir) != bool(args.properties):
        parser.error('--source-dir and --properties must be used together')
    if args.check and args.source_dir:
        parser.error('--check cannot import upstream files')
    if args.source_dir:
        import_source(args.source_dir, args.properties)
    rebuild(check=args.check)


if __name__ == '__main__':
    main()
