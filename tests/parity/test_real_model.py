"""Test real-model budget enforcement using a local upstream; no external calls."""
import json
import os
from pathlib import Path
import tempfile
import threading
import unittest
from unittest.mock import patch
import urllib.error
import urllib.request
from model_server import ModelServer
from real_model import RealModelServer


class BudgetTests(unittest.TestCase):
    def setUp(self):
        self.temp=tempfile.TemporaryDirectory()
        self.upstream=ModelServer()
        threading.Thread(target=self.upstream.serve_forever,daemon=True).start()
        with patch.dict(os.environ,{
            'NAST_GROUP_TEST_MODEL_URL':f'http://127.0.0.1:{self.upstream.server_port}/test/v1',
            'NAST_GROUP_TEST_MODEL':'test-model','NAST_GROUP_TEST_API_KEY':'never-persist-this-secret',
            'NAST_GROUP_TEST_REQUEST_LIMIT':'2'}):
            self.proxy=RealModelServer(Path(self.temp.name)/'requests.jsonl')
        threading.Thread(target=self.proxy.serve_forever,daemon=True).start()

    def tearDown(self):
        for server in [self.proxy,self.upstream]: server.shutdown();server.server_close()
        self.temp.cleanup()

    def request(self,body):
        req=urllib.request.Request(f'http://127.0.0.1:{self.proxy.server_port}/nast/v1/chat/completions',
            data=json.dumps(body).encode(),headers={'Content-Type':'application/json'})
        try:
            with urllib.request.urlopen(req) as response: return response.status,response.read()
        except urllib.error.HTTPError as error: return error.code,error.read()

    def test_shared_budget_streaming_and_no_credentials_in_evidence(self):
        body={'messages':[{'role':'user','content':'hello'}],'max_tokens':9999}
        self.assertEqual(self.request(body)[0],200)
        self.assertIn(b'[DONE]',self.request({**body,'stream':True})[1])
        self.assertEqual(self.request(body)[0],400)
        self.assertEqual(self.proxy.attempts,2)
        self.assertEqual(len(self.upstream.captures),2)
        self.assertTrue(all(c['body']['max_tokens']==512 for c in self.upstream.captures))
        for file in Path(self.temp.name).iterdir(): self.assertNotIn(b'never-persist-this-secret',file.read_bytes())

    def test_input_over_budget_never_reaches_upstream(self):
        self.assertEqual(self.request({'messages':[{'role':'user','content':'x'*8193}]})[0],400)
        self.assertEqual(self.proxy.attempts,0)
        self.assertEqual(len(self.upstream.captures),0)

    def test_failed_attempts_consume_budget(self):
        self.upstream.plans['test']=[{'status':503}]
        self.assertEqual(self.request({'messages':[{'role':'user','content':'hello'}]})[0],503)
        self.assertEqual(self.proxy.attempts,1)
        self.assertEqual(self.proxy.failures,[{'kind':'upstream','status':503,'attempt':1}])


if __name__=='__main__':unittest.main()
