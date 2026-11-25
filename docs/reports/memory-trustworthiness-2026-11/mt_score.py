import json,sys,random,collections
def load(path):
    d=json.load(open(path)); return d
def comps(sysrow):
    cat=collections.defaultdict(list)
    for q in sysrow['questions']: cat[q['category']].append(q['recall'])
    leaks={l['name']:l for l in sysrow['leaks']}
    return cat,leaks
POOLS={'Currency':['supersede'],'Recall':['retrieve','paraphrase','rephrase','stable'],'Private memory':['agent','owner','individual']}
def score(sysrow,draws=10000,seed=1):
    cat,leaks=comps(sysrow)
    vecs={k:[r for c in v for r in cat[c]] for k,v in POOLS.items()}
    n_leak=sum(leaks[k]['n'] for k in ('scope','agent')); ev=sum(leaks[k]['events'] or 0 for k in ('scope','agent'))
    # per-trial leak vector (events are counts of leak trials)
    lv=[1]*ev+[0]*(n_leak-ev)
    def point():
        acc=1-ev/n_leak
        c={k:sum(v)/len(v) for k,v in vecs.items()}
        return acc,c
    acc,c=point(); tot=(acc+sum(c.values()))/4
    rng=random.Random(seed); tots=[];parts={k:[] for k in ['Access']+list(vecs)}
    for _ in range(draws):
        a=1-sum(rng.choice(lv) for _ in lv)/len(lv)
        cs={k:sum(rng.choice(v) for _ in v)/len(v) for k,v in vecs.items()}
        parts['Access'].append(a)
        for k in cs: parts[k].append(cs[k])
        tots.append((a+sum(cs.values()))/4)
    def ci(x): x=sorted(x); return x[int(.025*len(x))],x[int(.975*len(x))-1]
    out={'n':{'Access':n_leak,**{k:len(v) for k,v in vecs.items()}},'Access':acc,**c,'score':tot,'score_ci':ci(tots),'ci':{k:ci(v) for k,v in parts.items()},'leak_events':ev}
    return out
if __name__=='__main__':
    d=load(sys.argv[1]); print(d['run']['stream_sha256'][:8],d['run']['replicates'],d['run'].get('skip_user_scope'))
    for s in d['systems']:
        o=score(s)
        print(s['system'], 'score %.3f (%.3f-%.3f)'%(o['score'],*o['score_ci']),' | '.join('%s %.2f (%.2f-%.2f) n=%d'%(k,o[k],*o['ci'][k],o['n'][k]) for k in ['Access','Currency','Recall','Private memory']),'leakEvents',o['leak_events'])
