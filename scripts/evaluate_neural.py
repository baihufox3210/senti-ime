#!/usr/bin/env python3
"""Held-out full-vocabulary next-token evaluation (not sampled accuracy)."""
from pathlib import Path
import argparse,json,struct
import numpy as np
from train_neural import read_corpus, data_arrays, softmax, TAG

ROOT=Path(__file__).resolve().parents[1]
parser=argparse.ArgumentParser(description=__doc__)
parser.add_argument('--model-dir',type=Path,default=ROOT/'models/tiny-ime-v2')
args=parser.parse_args()
out=args.model_dir
V,D,H,C,T,P=struct.unpack('<6I',(out/'model.bin').read_bytes()[8:32])
vocab=json.loads((out/'vocab.json').read_text())['tokens'];ids={t:i for i,t in enumerate(vocab)}
model=np.load(out/'training-checkpoint.npz')
data=data_arrays(read_corpus(ROOT/'target/training-corpus','test'),ids,np.ones(len(ids)),C,T)
x,topic,y,z=data
rng=np.random.default_rng(6511)
result={}
for language in ['english','chinese']:
    eligible=np.array([i>3 and (all(c.isascii() and (c.isalpha() or c=="'") for c in t) if language=='english' else len(t)==1 and '\u3400'<=t<='\u9fff') for i,t in enumerate(vocab)])
    positions=np.flatnonzero(eligible[y]);chosen=rng.choice(positions,min(2000,len(positions)),replace=False)
    predictions=[];confidence=[];actual=[]
    for start in range(0,len(chosen),128):
        ix=chosen[start:start+128];tw=model['topic_weights'][topic[ix]];tw/=np.maximum(tw.sum(1,keepdims=True),1e-6)
        inp=np.concatenate([model['E'][x[ix]].reshape(len(ix),-1),(model['E'][topic[ix]]*tw[:,:,None]).sum(1)],axis=1)
        h=np.tanh(inp@model['W']+model['b']);probs=softmax(h@model['O'].T+model['ob'])
        pos=softmax(h@model['P']+model['pb']);agreement=pos@model['lexpos'].T
        unknown=model['lexpos'].max(1)<.2;agreement[:,unknown]=.5
        probs[:,~eligible]=0;probs[agreement<.015]=0
        pred=probs.argmax(1);predictions.extend(pred.tolist());confidence.extend(probs[np.arange(len(ix)),pred].tolist());actual.extend(y[ix].tolist())
    predictions=np.array(predictions);confidence=np.array(confidence);actual=np.array(actual)
    thresholds={}
    for threshold in [.025,.05,.08,.12]:
        accepted=confidence>=threshold
        thresholds[str(threshold)]={'coverage':float(accepted.mean()),'accepted':int(accepted.sum()),'precision':float((predictions[accepted]==actual[accepted]).mean()) if accepted.any() else None}
    result[language]={'evaluated_known_word_positions':len(actual),'top1':float((predictions==actual).mean()),'thresholds':thresholds}
(out/'full-vocabulary-evaluation.json').write_text(json.dumps(result,indent=2)+'\n')
print(json.dumps(result,indent=2))
