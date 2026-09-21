"""Exercise saved-key validation through real ACP with an isolated profile and HTTP fixture."""
import http.server
import json
import os
from pathlib import Path
import queue
import subprocess
import sys
import tempfile
import threading

requests = []
expired = False

class Vendor(http.server.BaseHTTPRequestHandler):
    def log_message(self, *_):
        pass

    def do_GET(self):
        body = json.dumps({'data': [{'id': 'test-deployment'}]}).encode()
        self.send_response(200)
        self.send_header('Content-Type', 'application/json')
        self.end_headers()
        self.wfile.write(body)

    def do_POST(self):
        payload = json.loads(self.rfile.read(int(self.headers['Content-Length'])))
        key = self.headers.get('api-key')
        requests.append((self.path, key, payload.get('model')))
        valid = key == 'fixture-valid-key' and not expired
        self.send_response(200 if valid else 401)
        streaming = valid and payload.get('stream')
        self.send_header('Content-Type', 'text/event-stream' if streaming else 'application/json')
        self.end_headers()
        if streaming:
            chunk = {'id': 'fixture', 'object': 'chat.completion.chunk', 'created': 1,
                     'model': 'test-deployment',
                     'choices': [{'index': 0, 'delta': {'role': 'assistant', 'content': 'OK'}, 'finish_reason': None}]}
            self.wfile.write(('data: ' + json.dumps(chunk) + '\n\n').encode())
            chunk['choices'] = [{'index': 0, 'delta': {}, 'finish_reason': 'stop'}]
            self.wfile.write(('data: ' + json.dumps(chunk) + '\n\ndata: [DONE]\n\n').encode())
        else:
            self.wfile.write(json.dumps({
                'id': 'fixture',
                'choices': [{'message': {'role': 'assistant', 'content': 'OK'}, 'finish_reason': 'stop'}],
                'usage': {'prompt_tokens': 5, 'completion_tokens': 1, 'total_tokens': 6},
            } if valid else {'error': {'message': 'Expired API key', 'type': 'invalid_api_key'}}).encode())

class ACP:
    def __init__(self, binary, profile):
        env = {k: v for k, v in os.environ.items() if not k.startswith(('AZURE_', 'OPENAI_', 'GOOSE_'))}
        env.update(GOOSE_PATH_ROOT=str(profile), GOOSE_DISABLE_KEYRING='1')
        self.log = open(profile / 'acp-stderr.log', 'a')
        self.proc = subprocess.Popen([binary, 'acp'], stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                     stderr=self.log, text=True, env=env)
        self.messages = queue.Queue()
        def read():
            for line in self.proc.stdout:
                try:
                    self.messages.put(json.loads(line))
                except json.JSONDecodeError:
                    pass
        threading.Thread(target=read, daemon=True).start()
        self.counter = 0
        self.call('initialize', {'protocolVersion': 1, 'clientCapabilities': {},
                               'clientInfo': {'name': 'connection-fixture', 'version': '1'}})

    def call(self, method, params):
        self.counter += 1
        self.proc.stdin.write(json.dumps({'jsonrpc': '2.0', 'id': self.counter, 'method': method, 'params': params}) + '\n')
        self.proc.stdin.flush()
        while True:
            message = self.messages.get(timeout=45)
            if message.get('id') == self.counter:
                return message

    def close(self):
        self.proc.terminate()
        try:
            self.proc.wait(timeout=5)
        except subprocess.TimeoutExpired:
            self.proc.kill()
            self.proc.wait()
        self.log.close()

server = http.server.ThreadingHTTPServer(('127.0.0.1', 0), Vendor)
threading.Thread(target=server.serve_forever, daemon=True).start()
with tempfile.TemporaryDirectory(prefix='goose-provider-restart-') as directory:
    profile = Path(directory)
    client = ACP(str(Path(sys.argv[1]).resolve()), profile)
    prefix = '_goose/unstable/providers/config/'
    try:
        response = client.call(prefix + 'save', {'providerId': 'azure_openai', 'fields': [
            {'key': 'AZURE_OPENAI_ENDPOINT', 'value': f'http://127.0.0.1:{server.server_port}/openai/v1'},
            {'key': 'AZURE_OPENAI_DEPLOYMENT_NAME', 'value': 'test-deployment'},
            {'key': 'AZURE_OPENAI_API_KEY', 'value': 'fixture-valid-key'},
        ]})
        assert 'result' in response, response
        assert response['result']['status']['connectionChecked'], response
        assert requests[-1] == ('/openai/v1/chat/completions', 'fixture-valid-key', 'test-deployment'), requests
        rejected = client.call(prefix + 'save', {'providerId': 'azure_openai', 'fields': [
            {'key': 'AZURE_OPENAI_API_KEY', 'value': 'fixture-rejected-key'},
        ]})
        assert 'previous settings retained' in str(rejected), rejected
    finally:
        client.close()
    client = ACP(str(Path(sys.argv[1]).resolve()), profile)
    try:
        before = client.call(prefix + 'status', {'providerIds': ['azure_openai']})['result']['statuses'][0]
        assert before['isConfigured'] and not before['connectionChecked'], before
        after = client.call(prefix + 'status', {'providerIds': ['azure_openai'], 'checkConnections': True})['result']['statuses'][0]
        assert after['connectionChecked'] and not after.get('connectionError'), after
        assert after['testModel'] == 'test-deployment', after
        assert requests[-1][1] == 'fixture-valid-key', 'Rejected save replaced the prior key'
        expired = True
    finally:
        client.close()
    client = ACP(str(Path(sys.argv[1]).resolve()), profile)
    try:
        failed = client.call(prefix + 'status', {'providerIds': ['azure_openai'], 'checkConnections': True})['result']['statuses'][0]
        assert failed['isConfigured'] and failed['connectionChecked'] and failed['connectionError'], failed
        print(json.dumps({'save': 'authenticated inference', 'rejected_save': 'previous key retained',
                          'restart': 'fresh authenticated request', 'expired_after_restart': 'failure surfaced, settings retained',
                          'requests': len(requests), 'scope': 'real ACP and Azure adapter against local HTTP fixture'}))
    finally:
        client.close()
server.shutdown()
