"""Serialize independently authored byte expectations; never parse/execute source."""
from pathlib import Path
import copy
import hashlib
import json

HERE=Path(__file__).resolve().parent
cases=[]
def hx(text):return text.encode('utf-8').hex()
def read(site,path,text,policy,epoch=0):
    return dict(site=site,path=path,bytesHex=hx(text),newline=policy,sourceIdentity='self',filesystemEpoch=epoch)
def effect(site,kind,before,after,payload=None,reads=None,predecessors=None):
    r=dict(site=site,endpoint=kind,beforeHex=None if before is None else hx(before),afterHex=hx(after),
           consumedReads=reads or [],requiredEarlierCompletions=predecessors or [])
    if payload is not None:r.update(payloadHex=hx(payload),acceptedCharacterCount=len(payload),payloadByteCount=len(payload.encode()))
    return r
def add(id,category,purpose,source,sites,initial,reads,effects,barrier=None,refusal=None,algebra=None):
    sites={name:dict(text=text,startByte=len(source[:source.index(text)].encode()),endByte=len(source[:source.index(text)+len(text)].encode())) for name,text in sites.items()}
    row=dict(id=id,category=category,purpose=purpose,source=source,sourceSHA256=hashlib.sha256(source.encode()).hexdigest(),
        sites=sites,cwd='/authored/v17',initialFiles={k:None if v is None else hx(v) for k,v in initial.items()},
        suppliedReads=reads,expectedEffects=effects,expectedBarrier=barrier,expectedBindingRefusal=refusal,
        independentByteAlgebra=algebra,fixtureExecuted=False)
    cases.append(row);return row

s='with open("a", "r", encoding="utf-8", newline=None) as first:\n    normalized = first.read()\nwith open("b", "r", encoding="utf-8", newline="") as second:\n    preserved = second.read()\nwith open("out", "w", encoding="utf-8", newline="") as writer:\n    writer.write(normalized + preserved)\n'
add('P01','positive','Mixed source-derived policies on two captured reads',s,
    dict(readA='first.read()',readB='second.read()',openW='open("out", "w", encoding="utf-8", newline="")',write='writer.write(normalized + preserved)'),
    dict(a='A\r\nB\rC\n',b='D\r\nE\rF\n',out='old'),
    [read('readA','a','A\r\nB\rC\n','universal'),read('readB','b','D\r\nE\rF\n','preserve')],
    [effect('openW','successfulOpenTruncate','old',''),effect('write','logicalStreamProjection','','A\nB\nC\nD\r\nE\rF\n','A\nB\nC\nD\r\nE\rF\n',['readA','readB'],['readA','readB','openW'])],
    algebra='Normalize only a: CRLF→LF and lone CR→LF; concatenate untouched b after normalized a. w-open empties out first.')
s='with open("out", "r+", encoding="utf-8", newline="") as writer:\n    saved = writer.read()\n    writer.seek(0)\n    writer.write(saved.replace("go", "X", 1))\n'
p2=add('P02','positive','Full preserve-read, seek0 and shorter overlay retain suffix',s,
    dict(open='open("out", "r+", encoding="utf-8", newline="")',read='writer.read()',seek='writer.seek(0)',write='writer.write(saved.replace("go", "X", 1))'),
    dict(out='go\r\ntail\n'),[read('read','out','go\r\ntail\n','preserve')],
    [effect('write','logicalStreamProjection','go\r\ntail\n','X\r\ntail\n\n','X\r\ntail\n',['read'],['open','read','seek'])],
    algebra='Original9 bytes. Replacement payload8 bytes overlays bytes0..8; original byte8 LF remains. Not truncate/rewrite.')
s='with open("out", "r+", encoding="utf-8", newline="") as writer:\n    writer.write("é")\n'
p3=add('P03','positive','Zero-start multibyte write uses character acceptance and byte overlay',s,
    dict(open='open("out", "r+", encoding="utf-8", newline="")',write='writer.write("é")'),dict(out='old-tail'),[],
    [effect('write','logicalStreamProjection','old-tail','éd-tail','é',predecessors=['open'])],
    algebra='Payload U+00E9 is1 character/2 UTF-8 bytes; replace original bytes0..2 and preserve d-tail.')
p3['independentProofCorruptions']=[dict(change='acceptedCharacterCount=2',expected='reject_byte_count_as_character_count'),
    dict(change='acceptedCharacterCount=0',expected='reject_partial_acceptance_for_full_projection'),
    dict(change='current write evidence inserted as earlier completion',expected='reject_self_condition'),
    dict(change='future implicit close inserted as earlier completion',expected='reject_future_condition'),
    dict(change='later close fails or is unobserved',expected='logical_conditional_projection_only_never_file_realization_or_durability')]
s='with open("out", "r+", encoding="utf-8", newline="") as writer:\n    writer.write("")\n'
add('P04','positive','Present empty file and empty payload are valid no-op logical content',s,
    dict(open='open("out", "r+", encoding="utf-8", newline="")',write='writer.write("")'),dict(out=''),[],
    [effect('write','logicalStreamProjection','','','',predecessors=['open'])],algebra='Present zero-byte preimage + zero payload; no replacement ranges/diff. Still one source write effect.')
s='with open("out", "w", encoding="utf-8") as writer:\n    writer.write("é\\n")\n'
add('P05','legacy','No-newline legacy w remains truncate then close-anchored replacement',s,
    dict(open='open("out", "w", encoding="utf-8")',write='writer.write("é\\n")',close=s.rstrip()),dict(out='old'),[],
    [effect('open','successfulOpenTruncate','old',''),effect('close','legacyCloseFileProjection','','é\n','é\n',predecessors=['open','write'])],
    algebra='Legacy POSIX/LF runtime profile. No new logical effect at write anchor, no duplicate close effect.')
s='with open("out", "w", encoding="utf-8", newline="") as writer:\n    writer.write("A\\r\\nB\\rC\\n")\n'
p6=add('P06','positive','Explicit no-translation write preserves mixed payload endings',s,
    dict(open='open("out", "w", encoding="utf-8", newline="")',write='writer.write("A\\r\\nB\\rC\\n")'),dict(out='old'),[],
    [effect('open','successfulOpenTruncate','old',''),effect('write','logicalStreamProjection','','A\r\nB\rC\n','A\r\nB\rC\n',predecessors=['open'])],
    algebra='After successful truncation, encoded payload bytes unchanged; do not double CR before existing CRLF.')

def altered(id,base,purpose,refusal,change):
    row=copy.deepcopy(base);row.update(id=id,category='bindingRefusal',purpose=purpose,
        expectedBindingRefusal=refusal,derivedFrom=base['id'],changedInputOnly=change)
    # Retain source-effect expectations; exact materialization is withheld only for indicated sites.
    row['refusedSites']=['write'];cases.append(row);return row
q=altered('B01',p3,'Absent is not an empty update-file preimage','missingPresentPreimage','initialFiles.out absent');q['initialFiles']['out']=None
q=altered('B02',p2,'Read epoch cannot be replaced by logical target epoch','readBindingEpochMismatch','read epoch0→1');q['suppliedReads'][0]['filesystemEpoch']=1
q=altered('B03',p2,'Correct bytes under another source identity do not authorize read','readSourceIdentityMismatch','read source self→foreign');q['suppliedReads'][0]['sourceIdentity']='foreign'
q=altered('B04',p2,'Caller effective-read assertion contradicts explicit preserve policy','readNewlineContractMismatch','read preserve→universal');q['suppliedReads'][0]['newline']='universal'
q=altered('B05',p6,'Caller effective-write assertion cannot silently override explicit no-translation','writeNewlineContractMismatch','effective output claim CRLF');q['callerEffectiveWritePolicy']='crlf'
q=altered('B06',p3,'Valid input/payload can produce invalid UTF-8 retained suffix','invalidUtf8AfterOverlay','before euro-X, payload é');q['initialFiles']['out']=hx('€X');q['expectedEffects'][0]['beforeHex']=hx('€X');q['expectedEffects'][0]['afterHex']='c3a9ac58';q['independentByteAlgebra']='e282ac58 overlaid by c3a9 yields c3a9ac58; ac is an orphan continuation byte. Refuse textual exact preview, retain symbolic effect.'

for id,suffix,label in [('S01','    writer.write("Y")\n','second write'),('S02','    writer.read()\n','read after logical write'),('S03','    unknown()\n','opaque successor')]:
    source=p3['source']+suffix
    row=add(id,'prefix',label+' cannot consume/upgrade logical state',source,
        dict(open=p3['sites']['open']['text'],write=p3['sites']['write']['text'],barrier=suffix.strip()),
        dict(out='old-tail'),[],copy.deepcopy(p3['expectedEffects']),barrier='barrier',algebra=p3['independentByteAlgebra'])
    row['mustNotEmit']=['successor effects','implicit close projection','normal whole-script completion']

barriers=[
 ('S04','    writer.seek(1)\n    writer.write("X")\n','writer.seek(1)','Nonzero seek unsupported'),
 ('S05','    saved = writer.read()\n    writer.write("X")\n','writer.write("X")','Read followed by write requires explicit modeled seek0'),
 ('S06','    alias = writer\n    alias.write("X")\n','alias = writer','Handle alias remains outside selected contract'),
 ('S07','    cookie = writer.tell()\n    writer.seek(cookie)\n    writer.write("X")\n','writer.tell()','Opaque tell cookie remains unsupported'),
 ('S08','    with open("other", "w", encoding="utf-8", newline="") as nested:\n        nested.write("X")\n','with open("other", "w", encoding="utf-8", newline="") as nested:\n        nested.write("X")','Nested context remains unsupported'),
]
for id,body,barrier,purpose in barriers:
    source='with open("out", "r+", encoding="utf-8", newline="") as writer:\n'+body
    add(id,'semanticBarrier',purpose,source,dict(barrier=barrier),dict(out='old-tail',other='old'),[],[],barrier='barrier')
s='with open("out", "a", encoding="utf-8", newline="") as writer:\n    writer.write("X")\n'
add('S09','semanticBarrier','Append is not update at initial EOF',s,dict(barrier='open("out", "a", encoding="utf-8", newline="")'),dict(out='old-tail'),[],[],barrier='barrier')

s='with open("out", "w", encoding="utf-8", newline=None) as writer:\n    writer.write("é\\n")\n'
q=add('P07','positive','Explicit None newly logical uses declared runtime output separator',s,
    dict(open='open("out", "w", encoding="utf-8", newline=None)',write='writer.write("é\\n")'),dict(out='old'),[],
    [effect('open','successfulOpenTruncate','old',''),effect('write','logicalStreamProjection','','é\r\n','é\n',predecessors=['open'])],
    algebra='Accept2 source characters, UTF-8 payload3 bytes, runtime CRLF expands LF to CRLF: realized logical encoding4 bytes.')
q['runtimeDefaultWritePolicy']='crlf'
s='with open("a", "r", encoding="utf-8", newline="\\n") as first:\n    a = first.read()\nwith open("b", "r", encoding="utf-8", newline="\\r") as second:\n    b = second.read()\nwith open("c", "r", encoding="utf-8", newline="\\r\\n") as third:\n    c = third.read()\nwith open("out", "w", encoding="utf-8", newline="\\r\\n") as writer:\n    writer.write(a + b + c)\n'
add('P08','positive','Full reads preserve all explicit terminators; CRLF output translates every LF',s,
    dict(readA='first.read()',readB='second.read()',readC='third.read()',open='open("out", "w", encoding="utf-8", newline="\\r\\n")',write='writer.write(a + b + c)'),
    dict(a='A\r',b='B\n',c='C\r\n',out='old'),
    [read('readA','a','A\r','preserve'),read('readB','b','B\n','preserve'),read('readC','c','C\r\n','preserve')],
    [effect('open','successfulOpenTruncate','old',''),effect('write','logicalStreamProjection','','A\rB\r\nC\r\r\n','A\rB\nC\r\n',['readA','readB','readC'],['readA','readB','readC','open'])],
    algebra='Full read ignores line-iteration termination differences and preserves A-CR/B-LF/C-CRLF. Output replaces each LF with CRLF, preserving existing CR.')
s='with open("out", "w", encoding="utf-8", newline="\\r") as writer:\n    writer.write("\\n\\r\\n")\n'
add('P09','positive','Explicit CR output substitutes LF rather than normalizing existing CRLF',s,
    dict(open='open("out", "w", encoding="utf-8", newline="\\r")',write='writer.write("\\n\\r\\n")'),dict(out='old'),[],
    [effect('open','successfulOpenTruncate','old',''),effect('write','logicalStreamProjection','','\r\r\r','\n\r\n',predecessors=['open'])],
    algebra='Three accepted characters LF CR LF become CR CR CR, with3 bytes after UTF-8 encoding.')

for row in cases:
    row['contractVersion']='neutral-python-newline-logical-v1'
    if row['category']=='bindingRefusal':
        row['byteProjectionCreditAllowed']=False
        row['retainedByteExpectationMeaning']='counterfactual base-case algebra only; not an authorized preview for this corrupt or missing binding'
    row['nativeRequestMapping']='not_yet_authored; no native ids or ordinals inferred from outputs'
    row['callerReadPolicyMode']='sourceDeclared'
    row['defaultRuntimeWritePolicy']='lf'
    row['predecessorListMeaning']='minimum source prerequisites; exact full inventory must be source-reviewed in pre-capture adapter, never learned from predictions'
    row['premises']=['canonical unmodified Python standard APIs','UTF-8 strict','regular existing files except explicit absent probe','no concurrent mutation or path alias except same literal out','full accepted character payload for each logical projection','future filesystem realization requires successful flush/close; never durability']
with (HERE/'cases.jsonl').open('x') as f:
    for row in cases:f.write(json.dumps(row,ensure_ascii=False,sort_keys=True)+'\n')
with (HERE/'expectations.json').open('x') as f:
    json.dump(dict(schema='v17NeutralPythonContractPanel/v1',caseCount=len(cases),
        categoryCounts={k:sum(r['category']==k for r in cases) for k in sorted({r['category'] for r in cases})},
        holdout=False,primaryEvaluation='Original Fresh40 unchanged',nativeSchemaFrozen=False,
        goldEndpointsMustNotBeRelabeled=True,cases=cases),f,ensure_ascii=False,indent=2)
    f.write('\n')
