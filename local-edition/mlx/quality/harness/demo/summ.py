import json,sys,re
lab=sys.argv[1]
lastkey=None
for l in open('/Users/mihaiperdum/goose-builds/quality/DEMO-2026-09-28-strategy/turns.jsonl'):
    o=json.loads(l)
    if o['label']!=lab: continue
    c=o.get('composer')
    if c:
        c=re.sub(r'.*?(Remembered: \d+ earlier chats?|Loop this)','',c,count=1)
        c=c.replace('Loop DEMO-2026-09-28-strategy','').replace('Qwen3.8-27B-Atlassian-Q8-mlx','27B')[:330]
    g=o.get('glance'); g=g.replace('Qwen3.8-27B-Atlassian-Q8-mlx','27B').replace('Mihai-LeanZero/','')[:170] if g else g
    key=re.sub(r'\d','',(c or '')+'|'+(g or ''))+o['ev']
    if o['ev']=='change' and not c and key==lastkey: continue
    lastkey=key
    print(o['t'][11:19], o.get('secs',''), o['ev'], ('stop=%s'%o['stop']) if 'stop' in o else '', ('| C: '+c) if c else '', ('| G: '+g) if g else '', ('| cards: '+str(o['cards'])[:200]) if o.get('cards') else '', ('| DL: '+' || '.join(o['dl'])[:600]) if o.get('dl') else '', ('| tail: '+o['tail'][-300:]) if o.get('tail') else '')
