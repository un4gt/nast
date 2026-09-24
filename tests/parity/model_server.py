"""Deterministic OpenAI-compatible oracle transport (Python standard library).

This server records requests; it does not build prompts or decide ST parity.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import threading
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
        self.output = output


class Handler(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def log_message(self, *_):
        pass

    def send_json(self, status: int, body: object):
        data = canonical(body).encode()
        self.send_response(status)
        self.send_header("Content-Type", "application/json; charset=utf-8")
        self.send_header("Content-Length", str(len(data)))
        self.end_headers()
        self.wfile.write(data)

    def do_GET(self):
        if self.path.endswith("/models"):
            self.send_json(200, {"object": "list", "data": [
                {"id": "gpt-4o", "object": "model", "owned_by": "parity"}]})
        elif self.path == "/health":
            self.send_json(200, {"ok": True})
        elif self.path == "/captures":
            with self.server.lock:
                self.send_json(200, list(self.server.captures))
        else:
            self.send_json(404, {"error": {"message": "Unknown endpoint"}})

    def do_POST(self):
        if not self.path.endswith("/chat/completions"):
            self.send_json(404, {"error": {"message": "Unknown endpoint"}})
            return
        try:
            size = int(self.headers.get("Content-Length", "0"))
            if not 0 < size <= 16 * 1024 * 1024:
                raise ValueError("Invalid request size")
            raw = self.rfile.read(size).decode("utf-8")
            body = json.loads(raw)
            if not isinstance(body, dict) or not isinstance(body.get("messages"), list):
                raise ValueError("messages must be an array")
        except (ValueError, UnicodeError) as error:
            self.send_json(400, {"error": {"message": str(error)}})
            return
        answer = reply_for(body)
        capture = {"path": self.path, "raw": raw, "body": body, "reply": answer}
        with self.server.lock:
            self.server.captures.append(capture)
            if self.server.output:
                self.server.output.parent.mkdir(parents=True, exist_ok=True)
                with self.server.output.open("a", encoding="utf-8") as stream:
                    stream.write(canonical(capture) + "\n")
        if "/error/" in self.path:
            self.send_json(503, {"error": {"message": "Scripted model failure"}})
            return
        common = {"id": "chatcmpl-parity", "created": 0, "model": body.get("model", "gpt-4o")}
        if not body.get("stream", False):
            self.send_json(200, {**common, "object": "chat.completion", "choices": [{
                "index": 0, "message": {"role": "assistant", "content": answer,
                                        "reasoning_content": "核对上下文"},
                "finish_reason": "stop"}],
                "usage": {"prompt_tokens": 1, "completion_tokens": 1, "total_tokens": 2}})
            return
        self.send_response(200)
        self.send_header("Content-Type", "text/event-stream; charset=utf-8")
        self.send_header("Cache-Control", "no-cache")
        self.send_header("Connection", "close")
        self.end_headers()
        self.close_connection = True
        deltas = [{"role": "assistant", "reasoning_content": "核对上下文"}]
        deltas.extend({"content": answer[i:i + 3]} for i in range(0, len(answer), 3))
        try:
            for delta in deltas:
                frame = ("data: " + canonical({**common, "object": "chat.completion.chunk",
                    "choices": [{"index": 0, "delta": delta, "finish_reason": None}]}) + "\n\n").encode()
                # Deliberately split UTF-8 characters and SSE framing across writes.
                for start in range(0, len(frame), 7):
                    self.wfile.write(frame[start:start + 7])
                    self.wfile.flush()
                if "/abort/" in self.path:
                    return
            self.wfile.write(("data: " + canonical({**common, "choices": [
                {"index": 0, "delta": {}, "finish_reason": "stop"}]}) + "\n\ndata: [DONE]\n\n").encode())
            self.wfile.flush()
        except (BrokenPipeError, ConnectionResetError):
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
