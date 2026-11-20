#!/usr/bin/env python3
"""Prepare pinned public neural-training inputs; --fetch explicitly permits downloads."""
import argparse,hashlib,json,shutil,urllib.request
from pathlib import Path
root=Path(__file__).resolve().parents[1]
parser=argparse.ArgumentParser(description=__doc__)
parser.add_argument('--fetch',action='store_true')
args=parser.parse_args()
output=root/'target/training-corpus';output.mkdir(parents=True,exist_ok=True)
manifest=json.loads((root/'models/tiny-ime-v2/training-report.json').read_text())['sources']
for row in manifest:
    path=output/row['file']
    bundled=root/'models/tiny-ime-v2/corpus'/row['file']
    if not path.exists() and bundled.exists():shutil.copyfile(bundled,path)
    if not path.exists():
        if not args.fetch:raise SystemExit(f'Missing {path.name}; rerun with --fetch to download the pinned public source')
        with urllib.request.urlopen(row['url'],timeout=60) as response:data=response.read()
        if hashlib.sha256(data).hexdigest()!=row['sha256']:raise SystemExit(f'Source changed: {row["file"]}')
        path.write_bytes(data)
    if hashlib.sha256(path.read_bytes()).hexdigest()!=row['sha256']:raise SystemExit(f'Hash mismatch: {path.name}')
(output/'manifest.json').write_text(json.dumps(manifest,indent=2)+'\n')
print(f'Validated {len(manifest)} pinned public source files')
