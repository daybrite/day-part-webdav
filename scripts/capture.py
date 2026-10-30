#!/usr/bin/env python3
# Copyright © The Daybrite Project
# SPDX-License-Identifier: MPL-2.0
"""Capture synthetic DAV exchanges; optionally run Rust tests against both live servers.

macOS Apache is temporary and unprivileged. Nextcloud must be the isolated Lima fixture,
never a real account. Only a freshly created UUID collection is changed or deleted.
"""
from pathlib import Path
import argparse, base64, contextlib, grp, http.client, json, os, pwd, signal
import socket, subprocess, tempfile, time, urllib.parse, uuid

ROOT=Path(__file__).resolve().parents[1]
XML=b'<?xml version="1.0" encoding="utf-8"?><d:propfind xmlns:d="DAV:"><d:prop><d:resourcetype/><d:getetag/><d:getcontentlength/><d:getlastmodified/><d:displayname/><d:supported-report-set/></d:prop></d:propfind>'
SYNTHETIC_PASSWORD='synthetic-local-fixture-only'

@contextlib.contextmanager
def apache():
    with tempfile.TemporaryDirectory(prefix='day-part-webdav-') as tmp:
        root=Path(tmp);(root/'data').mkdir();(root/'locks').mkdir()
        with socket.socket() as s:s.bind(('127.0.0.1',0));port=s.getsockname()[1]
        modules=['mpm_event','unixd','authn_core','authn_file','authz_core','authz_user','auth_basic','dav','dav_fs']
        conf=f'ServerRoot "{root}"\nListen 127.0.0.1:{port}\nServerName localhost\nPidFile "{root}/pid"\nErrorLog "{root}/error.log"\n'
        conf+='\n'.join(f'LoadModule {m}_module /usr/libexec/apache2/mod_{m}.so' for m in modules)
        conf+=f'\nUser {pwd.getpwuid(os.getuid()).pw_name}\nGroup {grp.getgrgid(os.getgid()).gr_name}\nDocumentRoot "{root}/data"\nDavLockDB "{root}/locks/dav"\n'
        conf+=f'<Directory "{root}/data">\nDav On\nOptions None\nAllowOverride None\nAuthType Basic\nAuthName "Synthetic fixture"\nAuthUserFile "{root}/passwd"\nRequire valid-user\n</Directory>\n'
        (root/'httpd.conf').write_text(conf)
        subprocess.run(['/usr/sbin/htpasswd','-bc',str(root/'passwd'),'day_fixture',SYNTHETIC_PASSWORD],check=True,capture_output=True)
        with open(root/'process.log','w') as log:
            process=subprocess.Popen(['/usr/sbin/httpd','-f',str(root/'httpd.conf'),'-DFOREGROUND'],stdout=log,stderr=log,start_new_session=True)
            try:
                for _ in range(100):
                    try:
                        with socket.create_connection(('127.0.0.1',port),timeout=.1):break
                    except OSError:time.sleep(.05)
                yield f'http://127.0.0.1:{port}/',subprocess.check_output(['/usr/sbin/httpd','-v'],text=True).strip()
            finally:
                os.killpg(process.pid,signal.SIGTERM)
                try:process.wait(timeout=5)
                except subprocess.TimeoutExpired:os.killpg(process.pid,signal.SIGKILL);process.wait()

def run(name,url,version,native):
    parsed=urllib.parse.urlsplit(url)
    if parsed.hostname!='127.0.0.1' or parsed.scheme!='http':raise ValueError('fixture loopback only')
    auth='Basic '+base64.b64encode(f'day_fixture:{SYNTHETIC_PASSWORD}'.encode()).decode()
    def request(method,path,body=b'',headers=None):
        connection=http.client.HTTPConnection(parsed.hostname,parsed.port,timeout=20)
        h={'Authorization':auth,'Accept-Encoding':'identity'};h.update(headers or {})
        connection.request(method,path,body,h);r=connection.getresponse();data=r.read();hh=r.getheaders();status=r.status;connection.close()
        return status,hh,data
    root=parsed.path.rstrip('/')+'/day-webdav-fixture-'+uuid.uuid4().hex+'/'
    status,_,_=request('MKCOL',root)
    if status!=201:raise RuntimeError(('could not create fresh root',status))
    cases=[]
    def capture(label,method,relative,body=b'',headers=None):
        status,hh,data=request(method,root+relative,body,headers)
        # No cookies, authentication, account tokens, request IDs or real user content retained.
        allow={'content-type','content-length','etag','dav','allow','last-modified','retry-after','location','content-range'}
        safe_headers=[[k.lower(),v] for k,v in hh if k.lower() in allow]
        cases.append(dict(label=label,method=method,relative=relative,request_headers=headers or {},request_body=base64.b64encode(body).decode(),status=status,response_headers=safe_headers,body=base64.b64encode(data).decode()))
        return status,dict((k.lower(),v) for k,v in hh),data
    try:
        capture('options','OPTIONS','')
        capture('mkcol','MKCOL','items/')
        filename=urllib.parse.quote('café + 100%.bin',safe='+')
        path='items/'+filename
        capture('put','PUT',path,b'synthetic dav payload\x00\xff',{'If-None-Match':'*','Content-Type':'application/octet-stream'})
        capture('propfind','PROPFIND','items/',XML,{'Depth':'1','Content-Type':'application/xml; charset=utf-8'})
        status,h,_=capture('get','GET',path)
        capture('head','HEAD',path)
        capture('not-modified','GET',path,headers={'If-None-Match':h['etag']})
        capture('create-precondition','PUT',path,b'other',{'If-None-Match':'*','Content-Type':'application/octet-stream'})
        capture('stale-precondition','PUT',path,b'other',{'If-Match':'"stale-fixture"','Content-Type':'application/octet-stream'})
        capture('missing-parent','MKCOL','missing/child/')
        base=f'{parsed.scheme}://{parsed.netloc}{root}'
        capture('copy','COPY',path,headers={'Destination':base+'items/copy.bin','Overwrite':'F'})
        capture('copy-precondition','COPY',path,headers={'Destination':base+'items/copy.bin','Overwrite':'F'})
        capture('move','MOVE','items/copy.bin',headers={'Destination':base+'items/moved.bin','Overwrite':'F'})
        capture('delete','DELETE','items/moved.bin')
        capture('missing','GET','items/missing.bin')
        capture('stat','PROPFIND',path,XML,{'Depth':'0','Content-Type':'application/xml; charset=utf-8'})
        for _ in range(40):
            _,hh,_=request('GET',root+path);tag=dict((k.lower(),v) for k,v in hh)['etag']
            if not tag.startswith('W/'):break
            time.sleep(.1)
        if tag.startswith('W/'):raise RuntimeError('fixture did not supply a strong ETag for range probe')
        capture('range','GET',path,headers={'Range':'bytes=1-4','If-Range':tag})
        capture('range-restart','GET',path,headers={'Range':'bytes=1-4','If-Range':'"stale-fixture"'})
        doc=dict(server=name,version=version,base_url=base,fixture_only=True,captured_at=time.strftime('%Y-%m-%dT%H:%M:%SZ',time.gmtime()),redaction='Authorization, cookies and non-protocol headers omitted; bodies are synthetic server responses',cases=cases)
        destination=ROOT/'tests/fixtures'/f'{name}.json';destination.parent.mkdir(parents=True,exist_ok=True);destination.write_text(json.dumps(doc,indent=2)+'\n')
        print(name,version.splitlines()[0],[(c['label'],c['status']) for c in cases],flush=True)
        if native:
            env=os.environ.copy();env.update(DAV_FIXTURE_URL=base,DAV_FIXTURE_PASSWORD=SYNTHETIC_PASSWORD)
            subprocess.run(['cargo','test','--offline','--features','reqwest','--test','native','--','--ignored','--nocapture'],cwd=ROOT,env=env,check=True)
    finally:
        status,_,_=request('DELETE',root)
        if status not in (200,204):raise RuntimeError(('fixture cleanup failed',status))

if __name__=='__main__':
    parser=argparse.ArgumentParser();parser.add_argument('--native',action='store_true');args=parser.parse_args()
    with apache() as (url,version):run('apache',url,version,args.native)
    c=http.client.HTTPConnection('127.0.0.1',18443,timeout=10);c.request('GET','/status.php');version=json.loads(c.getresponse().read());c.close()
    run('nextcloud','http://127.0.0.1:18443/remote.php/dav/files/day_fixture/',version['versionstring'],args.native)
