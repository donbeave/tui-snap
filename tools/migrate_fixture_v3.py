#!/usr/bin/env python3
"""Export a reviewable migration of this known fixture's v2 approvals.

Never reads actual captures and never modifies approvals. The fixture source
hash proves the audited views cannot emit HIDDEN/SLOW_BLINK/RAPID_BLINK.
Continuation styles inherit their original lead only with an explicit flag;
this is a separate reviewed correction to the old adapter's dropped styles.
"""
import argparse, hashlib, json, subprocess, tempfile
from pathlib import Path
ROOT=Path(__file__).resolve().parents[1]
FIXTURE_SHA="ec70b5ad5480800bb9395961238beea47d924db15a355d8a5a9148ceccf5943f"
SOURCE="5036cf87e621e6beb66deffe3224abdbefc955cb"
def require(condition, message):
    if not condition: raise ValueError(message)
def git(*args):
    return subprocess.check_output(['git',*args],cwd=ROOT)
p=argparse.ArgumentParser();p.add_argument('--out',type=Path,required=True);p.add_argument('--continuation-styles',action='store_true');p.add_argument('--source-revision',default=SOURCE);args=p.parse_args()
require(args.source_revision==SOURCE,'source revision changed; reaudit before migration')
require(not args.out.exists() and not args.out.is_symlink(),'output already exists')
require(hashlib.sha256(git('show',f'{SOURCE}:examples/fixture_app.rs')).hexdigest()==FIXTURE_SHA,'fixture source changed; reaudit before migration')
paths=sorted(p for p in git('ls-tree','-r','--name-only',SOURCE,'tests/visual/approved').decode().splitlines() if p.endswith('.frame.json'))
require(len(paths)==24,'expected exactly 24 audited approvals')
ledger=[];outputs={}
for path in paths:
    original=git('show',f'{SOURCE}:{path}');frame=json.loads(original)
    require(frame['version']==2,'migration requires an untouched version-2 source')
    changed=[]
    for i,c in enumerate(frame['cells']):
        require('hidden' not in c['mods'] and 'blink' not in c['mods'],'unexpected source modifier')
        c['mods'].update(hidden=False,blink=False)
        if args.continuation_styles and c['continuation']:
            lead=frame['cells'][i-1]
            require(c['x']>0 and lead['width']==2 and lead['x']+1==c['x'] and lead['y']==c['y'],'invalid continuation')
            for field in ['fg','bg','mods']:
                if c[field]!=lead[field]:changed.append(dict(x=c['x'],y=c['y'],field=field,before=c[field],after=lead[field]))
                c[field]=lead[field]
    frame['version']=3
    output=json.dumps(frame,separators=(',',':'),ensure_ascii=False).encode()
    name=Path(path).name
    outputs[name]=output
    ledger.append(dict(file=name,before_sha256=hashlib.sha256(original).hexdigest(),after_sha256=hashlib.sha256(output).hexdigest(),continuation_changes=changed))
outputs['migration-ledger.json']=json.dumps(dict(source_revision=SOURCE,fixture_sha256=FIXTURE_SHA,entries=ledger),indent=2).encode()
args.out.parent.mkdir(parents=True,exist_ok=True)
with tempfile.TemporaryDirectory(prefix='tuisnap-migration-',dir=args.out.parent) as staging:
    destination=Path(staging)/'result';destination.mkdir()
    for name,data in outputs.items(): (destination/name).write_bytes(data)
    require(not args.out.exists() and not args.out.is_symlink(),'output appeared during migration')
    destination.rename(args.out)
print('exported',len(ledger),'frames from',SOURCE,'; approvals unchanged')
