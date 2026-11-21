#!/usr/bin/env python3
"""Train a small bilingual feed-forward neural LM and POS head using NumPy.

No user data. UD r2.15 train/dev/test remain separate; vocabulary uses train only.
Run with OPENBLAS_NUM_THREADS=2 python scripts/train_neural.py.
"""
import argparse, bz2, collections, hashlib, json, math, os, re, struct, time
from pathlib import Path
import numpy as np

ROOT = Path(__file__).resolve().parents[1]
POS = ['ADJ','ADP','ADV','AUX','CCONJ','DET','INTJ','NOUN','NUM','PART','PRON','PROPN','PUNCT','SCONJ','SYM','VERB','X','EOS']
TAG = {p:i for i,p in enumerate(POS)}
TOKEN = re.compile(r"[A-Za-z]+(?:'[A-Za-z]+)*|[0-9]+|[^\s]", re.UNICODE)
def tokenize(text):
    return ['<num>' if t.isdigit() else t.lower() for t in TOKEN.findall(text)]
def read_corpus(directory, split):
    result=[]
    for repo,prefix in [('UD_English-EWT','en_ewt'),('UD_Chinese-GSD','zh_gsd')]:
        sentence=[]
        for line in (directory/f'{repo}-{prefix}-ud-{split}.conllu').read_text().splitlines()+['']:
            if not line:
                if sentence: result.append(sentence);sentence=[]
            elif not line.startswith('#'):
                columns=line.split('\t')
                if columns[0].isdigit():
                    sentence.extend((t,TAG.get(columns[3],TAG['X'])) for t in tokenize(columns[1]))
    return result

def data_arrays(sentences, ids, frequencies, context, topic):
    x=[];history=[];y=[];pos=[]
    for sentence in sentences:
        previous=[ids['<bos>']]*context
        for token,tag in sentence+[('<eos>',TAG['EOS'])]:
            target=ids.get(token,ids['<unk>'])
            x.append(previous[-context:]);history.append(([ids['<bos>']]*topic+previous)[-topic:]);y.append(target);pos.append(tag)
            previous.append(target)
    return np.array(x,np.int32),np.array(history,np.int32),np.array(y,np.int32),np.array(pos,np.int32)
def softmax(logits):
    logits=logits-logits.max(axis=-1,keepdims=True)
    exp=np.exp(logits);return exp/exp.sum(axis=-1,keepdims=True)

def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--corpus',type=Path,default=ROOT/'target/training-corpus')
    parser.add_argument('--output',type=Path,default=ROOT/'models/tiny-ime-v2')
    parser.add_argument('--epochs',type=int,default=10)
    parser.add_argument('--examples-per-epoch',type=int,default=160000)
    parser.add_argument('--seed',type=int,default=4129)
    parser.add_argument('--embedding', type=int, default=32)
    parser.add_argument('--hidden', type=int, default=96)
    parser.add_argument('--context', type=int, default=6)
    parser.add_argument('--topic', type=int, default=12)
    parser.add_argument('--warm-start', type=Path)
    args=parser.parse_args()
    if min(args.embedding, args.hidden, args.context, args.topic, args.epochs, args.examples_per_epoch) < 1:
        parser.error('model dimensions and training sizes must be positive')
    rng=np.random.default_rng(args.seed)
    train=read_corpus(args.corpus,'train');dev=read_corpus(args.corpus,'dev');test=read_corpus(args.corpus,'test')
    # CC0 dialogue/prose supplement uses a deterministic text-hash split,
    # never importing held-out UD sentences into training.
    cc0=[[],[],[]]
    blocked={tuple(t for t,_ in sentence) for sentence in dev+test}
    cc0_path=args.corpus/'eng_sentences_CC0.tsv.bz2'
    if cc0_path.exists():
        seen=set()
        for line in bz2.decompress(cc0_path.read_bytes()).decode().splitlines():
            fields=line.split('\t')
            if len(fields)<3: continue
            ts=tokenize(fields[2]);key=tuple(ts)
            if not 3<=len(ts)<=80 or key in seen or key in blocked: continue
            seen.add(key)
            bucket=int(hashlib.sha256(' '.join(ts).encode()).hexdigest()[:8],16)%20
            cc0[1 if bucket==0 else 2 if bucket==1 else 0].append([(t,TAG['X']) for t in ts])
    dev+=cc0[1];test+=cc0[2]
    # Supplemental word readings teach character cohesion, not fake sentences.
    lexical=[]
    for name in ['zhuyin.tsv','zhuyin_extra.tsv']:
        for line in (ROOT/'crates/senti-predictors/data'/name).read_text().splitlines():
            if line and not line.startswith('#'):
                _,word,*_=line.split('\t')
                if len(word)>1: lexical.append([(c,TAG['X']) for c in word])
    # Deduplicate polyphonic spellings; do not duplicate validation/test sentences.
    unique={tuple(sentence) for sentence in lexical}
    train+= [list(s) for s in sorted(unique)]
    train+=cc0[0]
    domain=ROOT/'crates/senti-predictors/data/domain_sentences.txt'
    if domain.exists(): train += [[(t,TAG['X']) for t in tokenize(line)] for line in domain.read_text().splitlines() if line and not line.startswith('#')]
    counts=collections.Counter(t for sentence in train for t,_ in sentence)
    english=[t for t,n in counts.most_common() if re.fullmatch(r"[a-z]+(?:'[a-z]+)*",t) and n>=2][:6500]
    chinese=[t for t,n in counts.most_common() if len(t)==1 and '\u3400'<=t<='\u9fff'][:7000]
    punctuation=[t for t,n in counts.most_common() if len(t)==1 and not t.isalnum()][:64]
    vocab=['<bos>','<unk>','<eos>','<num>']+list(dict.fromkeys(english+chinese+punctuation))
    ids={t:i for i,t in enumerate(vocab)};V=len(vocab);D=args.embedding;H=args.hidden;C=args.context;T=args.topic;P=len(POS);BATCH=192;NEG=24
    frequencies=np.array([counts.get(t,1) for t in vocab],np.float32)
    weights=1/np.log2(frequencies+2);weights[:4]=0
    noise=frequencies**.75;noise[:2]=0;noise/=noise.sum()
    noise_log=np.log(np.maximum(noise,1e-9))
    datasets=[data_arrays(s,ids,frequencies,C,T) for s in [train,dev,test]]
    X,TOP,Y,Z=datasets[0]
    lexpos=np.full((V,P),.01,np.float32)
    for sentence in train:
        for token,tag in sentence:
            if token in ids and tag != TAG['X']: lexpos[ids[token],tag]+=1
    lexpos[2,TAG['EOS']]=100
    lexpos/=lexpos.sum(axis=1,keepdims=True)
    params={
        'E':rng.normal(0,.12,(V,D)).astype(np.float32),
        'W':rng.normal(0,.08,((C+1)*D,H)).astype(np.float32),
        'b':np.zeros(H,np.float32),
        'O':rng.normal(0,.08,(V,H)).astype(np.float32),
        'ob':np.log((frequencies+1)/(frequencies+1).sum()).astype(np.float32),
        'P':rng.normal(0,.08,(H,P)).astype(np.float32),
        'pb':np.zeros(P,np.float32),
    }
    if args.warm_start:
        base = args.warm_start
        tokens = json.loads((base/'vocab.json').read_text())['tokens']
        if tokens != vocab:
            parser.error('warm start requires identical vocabulary and order')
        _, old_d, old_h, old_c, _, old_p = struct.unpack('<6I', (base/'model.bin').read_bytes()[8:32])
        if D < old_d or H < old_h or C < old_c or P != old_p:
            parser.error('warm start requires equal or expanded compatible dimensions')
        with np.load(base/'training-checkpoint.npz') as old:
            params['E'][:, :old_d] = old['E']
            weights[:] = old['topic_weights']
            params['b'][:old_h] = old['b']
            params['W'][:, :old_h] = 0
            params['W'].reshape(C+1,D,H)[C-old_c:, :old_d, :old_h] = old['W'].reshape(old_c+1,old_d,old_h)
            params['O'][:, :old_h] = old['O']
            params['O'][:, old_h:] = 0
            params['P'][:old_h] = old['P']
            params['P'][old_h:] = 0
            params['ob'][:] = old['ob']
            params['pb'][:] = old['pb']
    moments={k:[np.zeros_like(v),np.zeros_like(v)] for k,v in params.items()}
    def forward(x,topic):
        tw=weights[topic];tw=tw/np.maximum(tw.sum(axis=1,keepdims=True),1e-6)
        bag=(params['E'][topic]*tw[:,:,None]).sum(axis=1)
        inp=np.concatenate([params['E'][x].reshape(len(x),-1),bag],axis=1)
        h=np.tanh(inp@params['W']+params['b'])
        return inp,h,tw
    eval_rng=np.random.default_rng(7701)
    eval_sets=[]
    for x,topic,y,z in datasets[1:]:
        eligible=np.flatnonzero(y!=1); chosen=eval_rng.choice(eligible,min(6000,len(eligible)),replace=False)
        candidates=eval_rng.choice(V,(len(chosen),NEG+1),p=noise);candidates[:,0]=y[chosen]
        eval_sets.append((x[chosen],topic[chosen],y[chosen],z[chosen],candidates))
    def evaluate(data):
        x,topic,y,z,candidates=data
        correct=pcorrect=loss=0
        baseline=0
        for start in range(0,len(y),256):
            sl=slice(start,start+256);_,h,_=forward(x[sl],topic[sl]);cand=candidates[sl]
            logits=np.einsum('bh,bkh->bk',h,params['O'][cand])+params['ob'][cand]-noise_log[cand]
            logits[:,1:][cand[:,1:]==y[sl,None]]=-30
            prob=softmax(logits)
            correct+=int((logits.argmax(1)==0).sum());loss+=float(-np.log(prob[:,0]+1e-9).sum())
            base=np.log(frequencies[cand])-noise_log[cand]
            baseline+=int((base.argmax(1)==0).sum())
            pred=(h@params['P']+params['pb']).argmax(1);mask=z[sl]!=TAG['X']
            pcorrect+=int(((pred==z[sl])&mask).sum())
        return {'sampled_accuracy':correct/len(y),'sampled_nll':loss/len(y),'unigram_sampled_accuracy':baseline/len(y),'pos_accuracy':pcorrect/max(1,int((z!=TAG['X']).sum()))}
    print(json.dumps({'vocab':V,'examples':[len(d[2]) for d in datasets],'validation_before':evaluate(eval_sets[0])}),flush=True)
    step=0;start_time=time.time();history=[]
    best_loss=float('inf');best_params=None
    for epoch in range(args.epochs):
        chosen=rng.choice(len(Y),min(args.examples_per_epoch,len(Y)),replace=False)
        running=0
        for start in range(0,len(chosen),BATCH):
            idx=chosen[start:start+BATCH];x=X[idx];topic=TOP[idx];y=Y[idx];z=Z[idx];n=len(idx)
            inp,h,tw=forward(x,topic)
            cand=rng.choice(V,(n,NEG+1),p=noise);cand[:,0]=y
            logits=np.einsum('bh,bkh->bk',h,params['O'][cand])+params['ob'][cand]-noise_log[cand]
            logits[:,1:][cand[:,1:]==y[:,None]]=-30
            prob=softmax(logits);running+=float(-np.log(prob[:,0]+1e-9).mean())
            g=prob;g[:,0]-=1;g/=n
            gradients={k:np.zeros_like(v) for k,v in params.items()}
            np.add.at(gradients['O'],cand,(g[:,:,None]*h[:,None,:]))
            np.add.at(gradients['ob'],cand,g)
            dh=np.einsum('bk,bkh->bh',g,params['O'][cand])
            pp=softmax(h@params['P']+params['pb']);pp[np.arange(n),z]-=1
            pp[z==TAG['X']]=0;pp*=.5/n
            gradients['P']=h.T@pp;gradients['pb']=pp.sum(0);dh+=pp@params['P'].T
            dh*=1-h*h
            gradients['W']=inp.T@dh;gradients['b']=dh.sum(0)
            di=dh@params['W'].T
            np.add.at(gradients['E'],x,di[:,:C*D].reshape(n,C,D))
            np.add.at(gradients['E'],topic,di[:,None,C*D:]*tw[:,:,None])
            step+=1;lr=(.00065 if args.warm_start else .0025)*(.85**epoch)
            for key,param in params.items():
                grad=np.clip(gradients[key],-1,1)
                m,v=moments[key];m*=.9;m+=.1*grad;v*=.999;v+=.001*grad*grad
                param-=lr*(m/(1-.9**step))/(np.sqrt(v/(1-.999**step))+1e-7)
        metrics=evaluate(eval_sets[0]);metrics.update(epoch=epoch+1,seconds=round(time.time()-start_time,1))
        history.append(metrics);print(json.dumps(metrics),flush=True)
        if metrics['sampled_nll']<best_loss:
            best_loss=metrics['sampled_nll'];best_params={k:v.copy() for k,v in params.items()}
    params=best_params
    args.output.mkdir(parents=True,exist_ok=True)
    arrays=[params[k] for k in ['E','W','b','O','ob','P','pb']]+[lexpos,weights]
    binary=b'SENTINN1'+struct.pack('<6I',V,D,H,C,T,P)+b''.join(a.astype('<f4').tobytes() for a in arrays)
    (args.output/'model.bin').write_bytes(binary)
    (args.output/'vocab.json').write_text(json.dumps({'tokens':vocab,'pos':POS},ensure_ascii=False,separators=(',',':'))+'\n')
    report={'architecture':f'context{C} + idf-topic{T} embedding{D} -> tanh{H} -> sampled-softmax word head + supervised POS head',
        'warm_start_sha256':hashlib.sha256((args.warm_start/'model.bin').read_bytes()).hexdigest() if args.warm_start else None,
        'seed':args.seed,'epochs':args.epochs,'examples_per_epoch':args.examples_per_epoch,'vocabulary':V,
        'examples':dict(zip(['train','dev','test'],[len(d[2]) for d in datasets])),
        'validation':evaluate(eval_sets[0]),'test':evaluate(eval_sets[1]),'history':history,
        'sources':json.loads((args.corpus/'manifest.json').read_text()),
        'lexicon_sources':{name:hashlib.sha256((ROOT/'crates/senti-predictors/data'/name).read_bytes()).hexdigest() for name in ['zhuyin.tsv','zhuyin_extra.tsv','domain_terms.tsv','domain_sentences.txt']},
        'numpy_version':np.__version__,
        'model_sha256':hashlib.sha256(binary).hexdigest(),'model_bytes':len(binary),
        'vocab_sha256':hashlib.sha256((args.output/'vocab.json').read_bytes()).hexdigest(),
        'oov_rates':{name:float((d[2]==1).mean()) for name,d in zip(['train','dev','test'],datasets)}}
    (args.output/'training-report.json').write_text(json.dumps(report,indent=2,ensure_ascii=False)+'\n')
    print(json.dumps({k:report[k] for k in ['validation','test','model_bytes']}),flush=True)
    np.savez(args.output/'training-checkpoint.npz',**params,lexpos=lexpos,topic_weights=weights)
if __name__=='__main__': main()
