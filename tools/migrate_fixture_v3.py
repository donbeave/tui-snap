#!/usr/bin/env python3
"""Export a reviewable migration of this known fixture's v2 approvals.

Never reads actual captures and never modifies approvals. The fixture source
hash proves the audited views cannot emit HIDDEN/SLOW_BLINK/RAPID_BLINK.
Continuation styles inherit their original lead only with an explicit flag;
this is a separate reviewed correction to the old adapter's dropped styles.
"""
import argparse, hashlib, json
from pathlib import Path
ROOT=Path(__file__).resolve().parents[1]
FIXTURE_SHA="ec70b5ad5480800bb9395961238beea47d924db15a355d8a5a9148ceccf5943f"
p=argparse.ArgumentParser();p.add_argument('--out',type=Path,required=True);p.add_argument('--continuation-styles',action='store_true');args=p.parse_args()
assert hashlib.sha256((ROOT/'examples/fixture_app.rs').read_bytes()).hexdigest()==FIXTURE_SHA,'fixture source changed; reaudit before migration'
args.out.mkdir(parents=True,exist_ok=True)
ledger=[]
for source in sorted((ROOT/'tests/visual/approved').glob('*.frame.json')):
    original=source.read_bytes();frame=json.loads(original)
    assert frame['version']==2,'migration requires an untouched version-2 source'
    changed=[]
    for i,c in enumerate(frame['cells']):
        assert 'hidden' not in c['mods'] and 'blink' not in c['mods']
        c['mods'].update(hidden=False,blink=False)
        if args.continuation_styles and c['continuation']:
            lead=frame['cells'][i-1]
            assert c['x']>0 and lead['width']==2 and lead['x']+1==c['x'] and lead['y']==c['y']
            for field in ['fg','bg','mods']:
                if c[field]!=lead[field]:changed.append(dict(x=c['x'],y=c['y'],field=field,before=c[field],after=lead[field]))
                c[field]=lead[field]
    frame['version']=3
    output=json.dumps(frame,separators=(',',':'),ensure_ascii=False).encode()
    target=args.out/source.name
    assert not target.exists(),'refusing to overwrite previous migration evidence'
    target.write_bytes(output)
    ledger.append(dict(file=source.name,before_sha256=hashlib.sha256(original).hexdigest(),after_sha256=hashlib.sha256(output).hexdigest(),continuation_changes=changed))
(args.out/'migration-ledger.json').write_text(json.dumps(dict(fixture_sha256=FIXTURE_SHA,entries=ledger),indent=2))
print('exported',len(ledger),'frames; approvals unchanged')
