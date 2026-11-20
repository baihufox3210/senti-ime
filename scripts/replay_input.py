#!/usr/bin/env python3
"""Replay authored input contracts against the real host, in ephemeral memory only."""
import argparse,json,subprocess
from pathlib import Path
parser=argparse.ArgumentParser(description=__doc__)
parser.add_argument('--binary',type=Path,default=Path('target/release/senti-host'))
parser.add_argument('--fixtures',type=Path,default=Path('testdata/input-contracts.json'))
args=parser.parse_args()
sequence=0
with subprocess.Popen([str(args.binary.resolve()),'--ephemeral'],stdin=subprocess.PIPE,stdout=subprocess.PIPE,text=True) as host:
 def send(command):
  global sequence
  sequence+=1
  host.stdin.write(json.dumps({'version':1,'id':sequence,'command':command},ensure_ascii=False)+'\n');host.stdin.flush()
  line=host.stdout.readline()
  if not line:raise RuntimeError('host exited before responding')
  response=json.loads(line)
  assert response.get('id')==sequence and response.get('ok'),response.get('error','protocol failure')
  return response
 cases=json.loads(args.fixtures.read_text())
 for case in cases:
  context='replay:'+case['name'];document=''
  response=send({'type':'activate','context':context,'mode':'Mixed','app':'Document'})
  response=send({'type':'memory_enabled','enabled':False})
  for number,step in enumerate(case['steps']):
   if 'text' in step:
    keys=[{'name':'space'} if c==' ' else {'name':'character','value':c} for c in step['text']]
   else:keys=[{'name':step['key']}]
   for key in keys:
    before=response.get('view') or {}
    response=send({'type':'key','context':context,'key':key})
    if key['name']=='enter' and before.get('composing') and not before.get('selecting'):
     assert response.get('commit')==before['converted_preedit'],(case['name'],'preview/Enter mismatch')
    document+=response.get('commit') or ''
    if not response.get('consumed'):
     document += ' ' if key['name']=='space' else key.get('value','')
   view=response.get('view') or {}
   actual={'preedit':view.get('converted_preedit'),'document':document,'selecting':view.get('selecting')}
   for key in ['preedit','document','selecting']:
    if key in step:assert actual[key]==step[key],(case['name'],number,key,step[key],actual[key])
   assert not (not view.get('composing') and view.get('suggestion')),(case['name'],'idle ghost')
  print('PASS',case['name'])
 host.stdin.close();host.wait(timeout=5)
 assert host.returncode==0
print(f'{len(cases)} authored input contracts passed; not an independent language accuracy benchmark')
