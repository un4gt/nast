"""Deterministic OpenAI-compatible oracle transport (Python standard library).

This server records requests; it does not build prompts or decide ST parity.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import threading
import socket
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path


def canonical(value: object) -> str:
    def numbers(item):
        if isinstance(item, float) and item.is_integer():
            return int(item)
        if isinstance(item, dict):
            return {key: numbers(value) for key, value in item.items()}
        if isinstance(item, list):
            return [numbers(value) for value in item]
        return item
    return json.dumps(numbers(value), ensure_ascii=False, sort_keys=True, separators=(",", ":"))


def reply_for(body: dict) -> str:
    semantic = {k: v for k, v in body.items() if k not in {"stream", "stream_options"}}
    return "已收到上下文：" + hashlib.sha256(canonical(semantic).encode()).hexdigest()


class ModelServer(ThreadingHTTPServer):
    daemon_threads = True

    def __init__(self, address=("127.0.0.1", 0), output: Path | None = None):
        super().__init__(address, Handler)
        self.captures: list[dict] = []
        self.lock = threading.Lock()
        self.plans: dict[str, list[dict]] = {}
        self.output = output
        self.models = ['gpt-4o']
        self.model_lists = []
        self.list_failures = 0


class Handler(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def setup(self):
        super().setup()
        self.connection.setsockopt(socket.IPPROTO_TCP, socket.TCP_NODELAY, 1)

    def log_message(self, *_):
        pass

    def send_json(self, status: int, body: object, headers=None):
        data = canonical(body).encode()
        self.send_response(status)
        self.send_header("Content-Type", "application/json; charset=utf-8")
        self.send_header("Content-Length", str(len(data)))
        for key, value in (headers or {}).items():
            self.send_header(key, str(value))
        self.end_headers()
        self.wfile.write(data)

    def do_GET(self):
        if self.path.endswith("/models"):
            self.server.model_lists.append({'path':self.path, 'credential_hash':hashlib.sha256(self.headers.get('Authorization','').encode()).hexdigest()})
            if self.server.list_failures:
                self.server.list_failures -= 1
                self.send_json(503, {'error':{'message':'Scripted model discovery failure'}})
                return
            self.send_json(200, {"object":"list", "data":[
                {"id":id,"object":"model","owned_by":"parity"} for id in self.server.models]})
        elif self.path == "/health":
            self.send_json(200, {"ok": True})
        elif self.path == "/captures":
            with self.server.lock:
                self.send_json(200, list(self.server.captures))
        else:
            self.send_json(404, {"error": {"message": "Unknown endpoint"}})

    def do_POST(self):
        protocol = 'anthropic' if self.path.endswith('/messages') else 'gemini' if ':generateContent' in self.path or ':streamGenerateContent' in self.path else 'openai'
        if protocol == 'openai' and not self.path.endswith('/chat/completions'):
            self.send_json(404, {'error': {'message': 'Unknown endpoint'}})
            return
        try:
            size = int(self.headers.get('Content-Length', '0'))
            if not 0 < size <= 16 * 1024 * 1024:
                raise ValueError('Invalid request size')
            raw = self.rfile.read(size).decode('utf-8')
            body = json.loads(raw)
            if not isinstance(body, dict) or not isinstance(body.get('contents' if protocol == 'gemini' else 'messages'), list):
                raise ValueError('messages/contents must be an array')
        except (ValueError, UnicodeError) as error:
            self.send_json(400, {'error': {'message': str(error)}})
            return
        answer = reply_for(body)
        route = self.path.split('/')[1]
        auth = self.headers.get('Authorization') or self.headers.get('x-api-key') or self.headers.get('x-goog-api-key') or ''
        capture = {'path': self.path, 'route': route, 'protocol': protocol, 'raw': raw, 'body': body, 'reply': answer,
                   'credential_hash': hashlib.sha256(auth.encode()).hexdigest()}
        with self.server.lock:
            self.server.captures.append(capture)
            plan = self.server.plans.get(route, [])
            action = plan.pop(0) if plan else {}
            if self.server.output:
                self.server.output.parent.mkdir(parents=True, exist_ok=True)
                with self.server.output.open('a', encoding='utf-8') as output:
                    output.write(canonical(capture) + '\n')
        time.sleep(action.get('delay', 0))
        status = action.get('status', 503 if '/error/' in self.path else 200)
        if status != 200:
            self.send_json(status, {'error': {'type': action.get('error_type','scripted_error'), 'message': action.get('message','Scripted model failure')}},
                           {'Retry-After': action['retry_after']} if 'retry_after' in action else None)
            return
        if action.get('invalid_json'):
            self.send_json(200, {'invalid': True})
            return
        stream = ':streamGenerateContent' in self.path if protocol == 'gemini' else body.get('stream', False)
        common = {'id': 'chatcmpl-parity', 'created': 0, 'model': body.get('model', 'gpt-4o')}
        if not stream:
            if protocol == 'anthropic':
                response = {'type':'message','content':[{'type':'text','text':answer},{'type':'thinking','thinking':'核对上下文'}], 'stop_reason':action.get('finish_reason','end_turn'),'usage':{'input_tokens':1,'output_tokens':1}}
            elif protocol == 'gemini':
                response = {'candidates':[{'content':{'parts':[{'text':'核对上下文','thought':True},{'text':answer}]},'finishReason':action.get('finish_reason','STOP')}]}
            else:
                response = {**common, 'object':'chat.completion','choices':[{'index':0,'message':{'role':'assistant','content':answer,'reasoning_content':'核对上下文'},'finish_reason':action.get('finish_reason','stop')}], 'usage':{'prompt_tokens':1,'completion_tokens':1,'total_tokens':2}}
            self.send_json(200,response)
            return
        self.send_response(200)
        self.send_header('Content-Type', 'text/event-stream; charset=utf-8')
        self.send_header('Cache-Control', 'no-cache')
        self.send_header('Connection', 'close')
        self.end_headers()
        self.close_connection = True
        cutoff = action.get('cutoff', 'reasoning' if '/abort/' in self.path else '')
        if cutoff == 'before':
            return
        def frame(content):
            wire = ('data: '+(content if isinstance(content,str) else canonical(content))+'\n\n').encode()
            for start in range(0,len(wire),7):
                self.wfile.write(wire[start:start+7]); self.wfile.flush()
        def delta(text, reasoning=False):
            if protocol == 'anthropic':
                return {'type':'content_block_delta','delta':{'type':'thinking_delta' if reasoning else 'text_delta', 'thinking' if reasoning else 'text':text}}
            if protocol == 'gemini':
                return {'candidates':[{'content':{'parts':[{'text':text,**({'thought':True} if reasoning else {})}]}}]}
            return {**common,'object':'chat.completion.chunk','choices':[{'index':0,'delta':{'reasoning_content' if reasoning else 'content':text},'finish_reason':None}]}
        try:
            if 'stream_error' in action:
                frame({'error':{'type':action['stream_error']}})
                return
            if not action.get('no_reasoning'):
                frame(delta('核对上下文',True))
            if cutoff == 'reasoning':
                return
            for index in range(0,len(answer),3):
                time.sleep(action.get('chunk_delay',0))
                frame(delta(answer[index:index+3]))
                if cutoff == 'body':
                    return
            if cutoff == 'no_done':
                return
            if protocol == 'anthropic':
                frame({'type':'message_delta','delta':{'stop_reason':action.get('finish_reason','end_turn')},'usage':{'output_tokens':1}})
                frame({'type':'message_stop'})
            elif protocol == 'gemini':
                frame({'candidates':[{'finishReason':action.get('finish_reason','STOP')}]})
            else:
                frame({**common,'choices':[{'index':0,'delta':{},'finish_reason':action.get('finish_reason','stop')}]})
                frame('[DONE]')
        except (BrokenPipeError, ConnectionResetError, ConnectionAbortedError):
            pass


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--port", type=int, default=19999)
    parser.add_argument("--output", type=Path)
    args = parser.parse_args()
    server = ModelServer(("127.0.0.1", args.port), args.output)
    print(f"http://127.0.0.1:{server.server_port}/v1", flush=True)
    try:
        server.serve_forever()
    except KeyboardInterrupt:
        pass
    finally:
        server.server_close()
