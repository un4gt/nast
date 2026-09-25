"""Validate a built image against disposable data, never a production volume."""
import argparse
import json
from pathlib import Path
import subprocess
import tempfile
import time
import uuid


def main(image):
    root=Path(tempfile.mkdtemp(prefix='nast-migration-container-'))
    user=root/'default-user';user.mkdir()
    settings={'oai_settings':{'custom_url':'http://localhost:19999/v1','custom_model':'legacy-model','chat_completion_source':'custom'}}
    secrets={'api_key_custom':[{'active':True,'value':'isolated-migration-test-key'}]}
    (user/'settings.json').write_text(json.dumps(settings),encoding='utf-8')
    (user/'secrets.json').write_text(json.dumps(secrets),encoding='utf-8')
    original_settings=(user/'settings.json').read_bytes()
    original_secrets=(user/'secrets.json').read_bytes()
    name='nast-model-migration-'+uuid.uuid4().hex[:10]
    subprocess.run(['docker','run','-d','--name',name,'-e','NAST_USERNAME=migration-test','-e','NAST_PASSWORD=isolated-test-password','--mount',f'type=bind,source={root},target=/app/data',image],check=True,capture_output=True)
    try:
        for _ in range(100):
            if (user/'models.json').exists(): break
            time.sleep(.1)
        first=(user/'models.json').read_bytes()
        catalog=json.loads(first);route=catalog['models'][0]['routes'][0]
        assert route['upstream_model']=='legacy-model'
        stored=json.loads((user/'secrets.json').read_text())
        assert stored[route['config']['credential_ref']][0]['value']=='isolated-migration-test-key'
        assert (user/'backups/pre-models-secrets.json').read_bytes()==original_secrets
        assert (user/'backups/pre-models-settings.json').read_bytes()==original_settings
        subprocess.run(['docker','restart',name],check=True,capture_output=True)
        # Verify the server actually finishes restarting, rather than merely observing old files.
        for _ in range(100):
            result=subprocess.run(['docker','exec',name,'curl','-fsS','http://127.0.0.1:8000/healthz'],capture_output=True)
            if result.returncode==0: break
            time.sleep(.1)
        assert result.returncode==0
        status=subprocess.check_output(['docker','exec',name,'curl','-s','-o','/dev/null','-w','%{http_code}','http://127.0.0.1:8000/ws']).decode()
        assert status=='401',status
        assert first==(user/'models.json').read_bytes()
        (root/'report.json').write_text(json.dumps({'image':image,'migration':True,'credentials':True,'backup':True,'restart_idempotent':True,'auth_required':True}),encoding='utf-8')
        print('PASS isolated container migration, credentials, exact backups and restart:',root)
    finally:
        subprocess.run(['docker','rm','-f',name],check=True,capture_output=True)


if __name__=='__main__':
    parser=argparse.ArgumentParser();parser.add_argument('--image',default='nast:model-routing-test')
    main(parser.parse_args().image)
