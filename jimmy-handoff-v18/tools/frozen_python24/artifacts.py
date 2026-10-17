"""Exact bounded endpoint artifacts; logical scope is not rewritten as physical."""
import check_artifact as independent


def expected(before, after):
    a,b=before.decode('utf8'),after.decode('utf8')
    if a==b:return [],''
    prefix=0
    for x,y in zip(a,b):
        if x!=y:break
        prefix+=len(x.encode())
    suffix=0
    for x,y in zip(a.encode()[prefix:].decode()[::-1],b.encode()[prefix:].decode()[::-1]):
        if x!=y:break
        suffix+=len(x.encode())
    replacement=[{'before':{'start':prefix,'end':len(before)-suffix},'after':{'start':prefix,'end':len(after)-suffix}}]
    old=independent.lf_lines(before);new=independent.lf_lines(after);p=0
    for x,y in zip(old,new):
        if x!=y:break
        p+=1
    s=0
    for x,y in zip(old[p:][::-1],new[p:][::-1]):
        if x!=y:break
        s+=1
    start=max(p-3,0);oe=min(len(old)-s+3,len(old));ne=min(len(new)-s+3,len(new));removed=oe-start;inserted=ne-start
    out='--- supplied-before\n+++ conditional-after\n'
    if not old and not new:return replacement,out
    out+=f'@@ -{start+bool(removed)},{removed} +{start+bool(inserted)},{inserted} @@\n'
    for mark,lines in [(' ',old[start:p]),('-',old[p:len(old)-s]),('+',new[p:len(new)-s]),(' ',old[len(old)-s:oe])]:
        for line in lines:
            out+=mark+line.decode()
            if not line.endswith(b'\n'):out+='\n\\ No newline at end of file\n'
    return replacement,out


def errors(preview,before,after):
    errors=[]
    try:
        if independent.bytes_field(preview['beforeBytes'],'before')!=before:errors.append('before_bytes')
        if independent.bytes_field(preview['afterBytes'],'after')!=after:errors.append('after_bytes')
        independent.check_replacements(before,after,preview.get('replacements'))
        if independent.apply_unified_diff(before,preview.get('unifiedDiff'))!=after:errors.append('diff_reconstruction')
        replacements,diff=expected(before,after)
        if preview.get('replacements')!=replacements:errors.append('exact_replacement')
        if preview.get('unifiedDiff')!=diff:errors.append('exact_diff')
    except (ValueError,KeyError,TypeError,UnicodeError) as exc:errors.append('artifact:'+str(exc))
    return errors
