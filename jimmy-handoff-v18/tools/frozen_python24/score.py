"""Saved-only V17 typed Python contract scorer. No analyzer/fixture execution."""
import argparse
import hashlib
import importlib.util
import json
from pathlib import Path
import sys

HERE=Path(__file__).resolve().parent
ROOT=HERE.parents[3]
WIRE=HERE.parent/'python-wire-v1'
CAPS=['standardPythonEvaluation','standardBuiltinOpen','standardTextMethods','noConcurrentMutation']


def need(value,message):
    if not value:raise ValueError(message)


def sha(raw):return hashlib.sha256(raw).hexdigest()
def canonical(value):return json.dumps(value,sort_keys=True,separators=(',',':'),ensure_ascii=True,allow_nan=False)
def same(a,b):return canonical(a)==canonical(b)


def pairs(items):
    obj={}
    for key,value in items:
        need(key not in obj,'duplicate JSON key');obj[key]=value
    return obj


def parse(raw):return json.loads(raw,object_pairs_hook=pairs,parse_constant=lambda x:(_ for _ in ()).throw(ValueError('nonfinite JSON')))


def read(path):
    path=path.absolute();need(not any(x.is_symlink() for x in [path,*path.parents]),'symlink input')
    need(path.is_file() and path.stat().st_size<=64*1024*1024,'input bound/type')
    return path.read_bytes()


def rows(raw):
    lines=raw.splitlines();need(0<len(lines)<=4096 and all(lines),'row bounds/blanks')
    return [parse(x) for x in lines]


def verify(pin):
    raw=read(HERE/'PINS.json');need(sha(raw)==pin,'packet SHA')
    manifest=parse(raw);need(manifest['readyForScoring'] is True,'packet not sealed for scoring')
    for f in manifest['files']:
        need(not Path(f['path']).is_absolute() and '..' not in Path(f['path']).parts,'pin path')
        data=read(ROOT/f['path']);need(len(data)==f['bytes'] and sha(data)==f['sha256'],'pin differs: '+f['path'])
    sys.dont_write_bytecode=True;sys.path.insert(0,str(HERE))
    import artifacts
    return artifacts


def rejected(record,cid):
    need(record.get('schema')=='conditionalSnapshotPreviewCapture/v1','rejected schema')
    need(set(record) in ({'schema','construction','reason'},{'schema','construction','reason','id'}),'rejected envelope keys')
    need(record.get('id',cid)==cid and isinstance(record.get('reason'),str),'rejected identity/reason')
    reason=record['reason']
    # Pinned serde constructor: idless records are bound by exact input order,
    # never counted as native semantic refusals.
    if ('unknown variant' in reason and 'sourceDeclared' in reason) or ('unknown field' in reason and 'logicalStream' in reason):
        return 'interfaceUnavailable'
    return 'constructionRejected'


def consumed_reads(request,operation):
    found=[]
    def walk(value):
        if isinstance(value,dict):
            if value.get('type')=='read':
                identity=(value['evidence']['sourceId'],value['readId'])
                if identity not in found:found.append(identity)
            for v in value.values():walk(v)
        elif isinstance(value,list):
            for v in value:walk(v)
    walk(operation.get('content'))
    result=[]
    for sid,rid in found:
        matches=[x for x in request['reads'] if (x['sourceId'],x['readId'])==(sid,rid)]
        need(len(matches)==1,'expected read binding inventory')
        x=matches[0];result.append({'read':{'sourceId':sid,'readId':rid},'filesystemEpoch':x['filesystemEpoch'],
            'target':{'type':'literal','path':x['path'],'cwd':x['cwd']},'encoding':x['encoding'],'newline':x['newline']})
    return result


def preview_errors(request,effect,edit,preview,artifact):
    pred=edit['details']['prediction'];op=pred['operation'];ordinal=edit['ordinal'];t=request['targets'][ordinal]
    logical=effect['endpoint']=='logicalStreamProjection';before=bytes.fromhex(effect['beforeHex']);after=bytes.fromhex(effect['afterHex'])
    expected={'basis':'suppliedSnapshotConditional','scope':'logicalStream' if logical else 'operationPoint',
        'callId':request['id'],'ordinal':ordinal,'evidence':edit['evidence'],'target':pred['target'],
        'filesystemEpoch':t['filesystemEpoch'],'readBindings':consumed_reads(request,op),'textProfile':request['textProfile'],
        'details':{'language':'python','assumptions':CAPS,'finalState':pred['finalState'],'iterationContext':[]},
        'carrierAssumptions':[],'predecessorConditions':pred['predecessorConditions'],'laterContext':edit['laterContext']}
    if logical:
        payload=bytes.fromhex(effect['payloadHex']).decode('utf8')
        if op['newline']=='cr':payload=payload.replace('\n','\r')
        elif op['newline']=='crLf' or (op['newline']=='runtimeDefault' and request['textProfile']['writeNewline']=='crLf'):payload=payload.replace('\n','\r\n')
        expected['logicalStream']={'binding':t['logicalStream'],'mode':op['mode'],'position':op['position'],
            'revisionAfter':1,'payloadBytes':list(payload.encode()),'acceptedCharactersRequired':effect['acceptedCharacterCount'],
            'realization':'requiresSuccessfulFlushOrClose'}
    errors=['preview_'+k for k,v in expected.items() if not same(preview.get(k),v)]
    if not logical and 'logicalStream' in preview:errors.append('physical_has_logical_metadata')
    need(set(preview)==set(expected)|{'beforeBytes','afterBytes','replacements','unifiedDiff'},'preview closed keys')
    return errors+artifact.errors(preview,before,after)


REFUSALS={
    'B01':('refused',{'type':'missingLogicalStreamPreimage'}),
    'B02':('refused',{'type':'readBindingMismatch','read':{'sourceId':0,'readId':0}}),
    'B03':('invalidBinding','snapshot names a source outside this analysis'),
    'B04':('refused',{'type':'readBindingMismatch','read':{'sourceId':0,'readId':0}}),
    'B05':('refused',{'type':'newlineProfileMismatch'}),
    'B06':('refused',{'type':'invalidUtf8'}),
}


def assess(request,spec,envelope,artifact):
    cid=request['id'];need(isinstance(envelope,dict) and isinstance(envelope.get('result'),dict),'snapshot envelope')
    record=envelope['result'];row={'id':cid,'category':spec['category'],'errors':[],'effects':[],'semanticBarrierExact':False,'layer':None}
    if record.get('construction')=='rejected':row['layer']=rejected(record,cid);return row
    need(record.get('schema')=='conditionalSnapshotPreviewCapture/v1' and record.get('id')==cid and record.get('construction')=='accepted','record identity')
    a=record.get('analysis');need(isinstance(a,dict) and a.get('callId')==cid,'analysis identity')
    if a.get('disposition')!='analyzed':
        row['layer']='analysisUnavailable';row['analysisDisposition']=a.get('disposition')
        need(isinstance(a.get('disposition'),dict) and set(a['disposition'])=={'unavailable'} and a['disposition']['unavailable'] in ['unsupportedSource','timeBudget','sourceLimit','unsupportedLanguage'],'unknown analysis disposition')
        need(all(a.get(k)==[] for k in ['sources','edits','barriers','pythonBarriers']) and a.get('javascriptBarriers',[])==[] and record.get('sourceTexts')==[],'fallback leakage')
        need(same(record.get('previews'),[{'ordinal':t['ordinal'],'disposition':'invalidBinding','reason':'snapshot names a source outside this analysis'} for t in request['targets']]),'fallback outcome inventory')
        return row
    row['layer']='analyzed'
    need(record.get('sourceTexts')==[request['source']] and same(a.get('sources'),[{'id':0,'language':'python','parent':None}]),'source identity')
    need(sha(request['source'].encode())==spec['sourceSha256'],'proof source SHA')
    if not same(a.get('edits'),spec['expectedEdits']):row['errors'].append('exact_source_edits')
    if a.get('barriers')!=[] or a.get('javascriptBarriers',[])!=[]:row['errors'].append('wrong_language_barriers')
    if not same(a.get('pythonBarriers'),spec['expectedBarriers']):row['errors'].append('exact_python_barriers')
    outcomes=record.get('previews');need(isinstance(outcomes,list) and same([x.get('ordinal') for x in outcomes],[x['ordinal'] for x in request['targets']]),'outcome inventory/order')
    if spec['category']=='semanticBarrier':row['semanticBarrierExact']=not row['errors'];return row
    for effect,edit,outcome in zip(spec['effects'],spec['expectedEdits'],outcomes,strict=True):
        problems=[];expected_refusal=REFUSALS.get(cid) if effect.get('site') in spec.get('refusedSites',[]) else None
        if expected_refusal:
            disposition,reason=expected_refusal
            if not same(outcome,{'ordinal':edit['ordinal'],'disposition':disposition,'reason':reason}):problems.append('typed_refusal_layer')
        elif outcome.get('disposition')!='preview':problems.append('missing_preview')
        else:
            need(set(outcome)=={'disposition','ordinal','preview'},'preview outcome keys')
            problems+=preview_errors(request,effect,edit,outcome['preview'],artifact)
        row['effects'].append({'operationId':effect.get('originalId'),'endpoint':effect['endpoint'],
            'sourceExact':not row['errors'],'exactConditional':not row['errors'] and not problems and expected_refusal is None,
            'typedRefusalExact':not row['errors'] and not problems and expected_refusal is not None,'errors':problems})
    return row


def load_module(name,path):
    spec=importlib.util.spec_from_file_location(name,path)
    module=importlib.util.module_from_spec(spec);spec.loader.exec_module(module);return module


def old_context():
    old=load_module('v16_fresh_saved_score',HERE.parent.parent/'coverage-v16/fresh40-scoring-v2/score.py')
    common,binder,scorer,cases,bindings,requests,facts,contract=old.initialize('6fc271fee7af3ebf5bb982d11e97d9e8df9d7348b4ead23894023d52bfd8e264')
    bindings,requests,facts=old.expand(common,binder,cases,bindings,requests,facts,contract)
    old.install_strict_expansion(common,scorer,cases)
    adapter=load_module('v16_append_saved_adapter',old.HERE/'append_adapter.py')
    bindings,requests,facts,proofs=adapter.expand_append(common,binder,cases,bindings,requests,facts,contract,read=old.read,original=old.ORIGINAL)
    adapter.install_append(common,scorer,proofs)
    return scorer,cases,bindings,requests,facts,contract


def summarize(requests,specs,responses,artifact,panel):
    need(len(requests)==len(responses),'response row count')
    results=[];old=None
    if panel=='fresh40':old=old_context()
    for request,envelope in zip(requests,responses,strict=True):
        cid=request['id']
        if cid in specs:
            try:result=assess(request,specs[cid],envelope,artifact)
            except (ValueError,KeyError,TypeError,AttributeError,IndexError) as error:
                result={'id':cid,'layer':'responseError','errors':[str(error)],'effects':[],'semanticBarrierExact':False,'category':specs[cid]['category']}
            # Never lose authored denominator when transport or preview is missing.
            assessed={x.get('operationId'):x for x in result['effects']}
            if panel=='fresh40':
                result['originalOperations']=[{'operationId':e['originalId'],'endpoint':e['endpoint'],
                    'status':'conditionalLogical' if assessed.get(e['originalId'],{}).get('exactConditional') and e['endpoint']=='logicalStreamProjection'
                    else 'conditionalPhysical' if assessed.get(e['originalId'],{}).get('exactConditional') else result['layer'] if result['layer']!='analyzed' else 'miss'} for e in specs[cid]['effects']]
        else:
            need(old is not None,'missing source proof')
            scorer,cases,bindings,original_requests,facts,contract=old
            need(same(request,original_requests[cid]),'unreviewed old-lane request mutation')
            try:result=scorer.score_case(cases[cid],bindings[cid],request,facts[cid],contract,envelope,scorer.endpoint_checker())
            except (ValueError,KeyError,TypeError,AttributeError,IndexError) as error:
                result=scorer.incomplete_case(cases[cid],bindings[cid],'response_error',str(error),envelope=envelope)
            result={'id':cid,'layer':'frozenV16Scorer','preservedAssessment':result}
        results.append(result)
    totals={}
    for category in sorted({specs[r['id']]['category'] for r in results if r['id'] in specs}):
        members=[r for r in results if r['id'] in specs and specs[r['id']]['category']==category]
        totals[category]={'cases':len(members),'layers':{layer:sum(r['layer']==layer for r in members) for layer in sorted({r['layer'] for r in members})},
            'semanticBarriersExact':sum(r.get('semanticBarrierExact',False) for r in members),'endpoints':{}}
        for endpoint in sorted({e['endpoint'] for r in members for e in specs[r['id']]['effects']}):
            denominator=sum(e['endpoint']==endpoint for r in members for e in specs[r['id']]['effects'])
            totals[category]['endpoints'][endpoint]={'authored':denominator,
                'exactConditional':sum(e['endpoint']==endpoint and e['exactConditional'] for r in members for e in r.get('effects',[])),
                'typedRefusalExact':sum(e['endpoint']==endpoint and e['typedRefusalExact'] for r in members for e in r.get('effects',[]))}
    if panel=='fresh40':
        operations=[op for r in results for op in (r.get('originalOperations',[]) or r.get('preservedAssessment',{}).get('operations',[]))]
        need(len(operations)==27 and len({x['operationId'] for x in operations})==27,'original 27 denominator')
        totals['originalPrimitives']={'all':27,'python':14,'javascript':13,'operations':operations}
        totals['negativeAssessments']=[r['preservedAssessment'] for r in results if r.get('preservedAssessment',{}).get('negative') is not None]
        need(len(totals['negativeAssessments'])==20,'original negative denominator')
    return {'results':results,'totals':totals}


def source_status(requests,responses):
    need(len(requests)==len(responses),'source row count');out=[]
    for request,record in zip(requests,responses,strict=True):
        need(record.get('schema')=='selectiveEditsCapture/v1' and record.get('id')==request['id'],'direct source identity/schema')
        a=record.get('analysis');need(record.get('construction')=='accepted' and isinstance(a,dict) and a.get('callId')==request['id'],'source admission')
        if a.get('edits'):need(record.get('sourceTexts')==[request['source']],'source bytes')
        out.append({'id':request['id'],'sourceSha256':sha(request['source'].encode()),'disposition':a.get('disposition'),
            'pythonBarriers':a.get('pythonBarriers'),'operations':[e.get('details',{}).get('prediction',{}).get('operation',{}).get('type') for e in a.get('edits',[])],
            'semanticOrEndpointCredit':False})
    return {'results':out,'semanticOrEndpointCredit':False}


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--panel',choices=['fresh40','panel','fresh40-source','panel-source'],required=True)
    parser.add_argument('--version',choices=['v16','v17'],required=True)
    for field in ['responses','output','identity']:parser.add_argument('--'+field,type=Path,required=True)
    for field in ['pins-sha256','responses-sha256','identity-sha256']:parser.add_argument('--'+field,required=True)
    args=parser.parse_args();artifact=verify(args.pins_sha256)
    raw=read(args.responses);need(sha(raw)==args.responses_sha256,'response SHA')
    identity_raw=read(args.identity);need(sha(identity_raw)==args.identity_sha256,'identity SHA');identity=parse(identity_raw)
    filename={'fresh40':'fresh40-requests.jsonl','panel':'panel-requests.jsonl','fresh40-source':'fresh40-python-source.jsonl','panel-source':'panel-source.jsonl'}[args.panel]
    request_raw=read(WIRE/filename);requests=rows(request_raw)
    need(identity['version']==args.version and identity['requestsSha256']==sha(request_raw) and identity['responsesSha256']==sha(raw),'identity linkage')
    for value,length in [(identity['candidateCommit'],40),(identity['candidateManifestSha256'],64),(identity['binary']['sha256'],64)]:
        need(isinstance(value,str) and len(value)==length and all(c in '0123456789abcdef' for c in value),'identity format')
    specs={s['id']:s for s in parse(read(WIRE/'source-proofs.json'))['specs']}
    report=source_status(requests,rows(raw)) if args.panel.endswith('-source') else summarize(requests,specs,rows(raw),artifact,args.panel)
    report.update({'schema':'v17PythonSavedEvidence/v1','panel':args.panel,'version':args.version,'packetSha256':args.pins_sha256,
        'requestsSha256':sha(request_raw),'responsesSha256':sha(raw),'identitySha256':args.identity_sha256,
        'identity':identity,'receiptLimit':'Saved bytes and declared identities only; no execution or receipt attestation.',
        'fixtureExecuted':False,'populationCoverageClaim':False,'logicalResultsArePhysicalExactEdits':False})
    with args.output.open('x',encoding='utf8') as stream:json.dump(report,stream,indent=2,allow_nan=False);stream.write('\n')


if __name__=='__main__':main()
