"""Budgeted, isolated OpenAI-compatible proxy for optional real-model acceptance.

Credentials remain in this process, never in application data or captured headers.
Every upstream attempt, including retries by either application, consumes the budget.
"""
import json
import os
import threading
import urllib.error
import urllib.request
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path


def input_upper_bound(body):
    # Conservative byte bound includes message framing and the whole JSON request.
    # For byte-level OpenAI BPE, a token cannot contain less than one byte.
    return len(json.dumps(body,ensure_ascii=False).encode()) + 32*len(body.get('messages',[])) + 128


class RealModelServer(ThreadingHTTPServer):
    daemon_threads=True
    real_model=True

    def __init__(self, output: Path):
        required=['NAST_GROUP_TEST_MODEL_URL','NAST_GROUP_TEST_MODEL','NAST_GROUP_TEST_API_KEY']
        missing=[name for name in required if not os.environ.get(name)]
        if missing: raise RuntimeError('Missing independent test configuration: '+', '.join(missing))
        self.endpoint=os.environ[required[0]].rstrip('/')+'/chat/completions'
        self.model=os.environ[required[1]]
        self.key=os.environ[required[2]]
        self.limit=min(1000,max(1,int(os.environ.get('NAST_GROUP_TEST_REQUEST_LIMIT','1000'))))
        self.attempts=0;self.captures=[];self.failures=[];self.lock=threading.Lock();self.output=output
        super().__init__(('127.0.0.1',0),RealHandler)


class RealHandler(BaseHTTPRequestHandler):
    protocol_version='HTTP/1.0'

    def log_message(self,*args): pass

    def respond(self,status,body):
        payload=json.dumps(body).encode()
        self.send_response(status);self.send_header('Content-Type','application/json')
        self.send_header('Content-Length',str(len(payload)));self.end_headers();self.wfile.write(payload)

    def do_GET(self):
        self.respond(200,{'data':[{'id':'gpt-4o','object':'model'}]})

    def do_POST(self):
        body=json.loads(self.rfile.read(int(self.headers.get('Content-Length','0'))))
        body['model']=self.server.model
        body['max_tokens']=min(512,int(body.get('max_tokens',512)))
        upper=input_upper_bound(body)
        with self.server.lock:
            if self.server.attempts>=self.server.limit or upper>8192:
                self.server.failures.append({'kind':'budget','input_upper_bound':upper,'attempts':self.server.attempts})
                self.respond(400,{'error':{'message':'Independent acceptance budget exceeded'}})
                return
            self.server.attempts+=1
            number=self.server.attempts
            capture={'path':self.path,'body':body,'attempt':number,'input_upper_bound':upper}
            self.server.captures.append(capture)
            with self.server.output.open('a',encoding='utf-8') as file: file.write(json.dumps(capture,ensure_ascii=False)+'\n')
        request=urllib.request.Request(self.server.endpoint,data=json.dumps(body).encode(),headers={
            'Authorization':'Bearer '+self.server.key,'Content-Type':'application/json'})
        try:
            response=urllib.request.urlopen(request,timeout=90)
        except (urllib.error.URLError,TimeoutError) as error:
            status=error.code if isinstance(error,urllib.error.HTTPError) else 502
            # Do not persist exception text, remote headers, URLs or error bodies containing credentials.
            with self.server.lock: self.server.failures.append({'kind':'upstream','status':status,'attempt':number})
            self.respond(status,{'error':{'message':'Independent model service failed','status':status}})
            return
        with response, (self.server.output.parent/f'real-response-{number:04}.bin').open('wb') as file:
            self.send_response(response.status)
            self.send_header('Content-Type',response.headers.get('Content-Type','application/json'))
            self.end_headers()
            while chunk:=response.read1(4096):
                file.write(chunk)
                try: self.wfile.write(chunk);self.wfile.flush()
                except (ConnectionError,OSError): break
