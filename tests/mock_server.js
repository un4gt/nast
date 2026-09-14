// 常驻 mock OpenAI 兼容服务（19999）：SSE 流式回显
const http = require('http');
const server = http.createServer((req, res) => {
  let body = '';
  req.on('data', (c) => (body += c));
  req.on('end', () => {
    const j = JSON.parse(body);
    const last = j.messages[j.messages.length - 1];
    const reply = 'echo: ' + String(last.content || '').slice(0, 60);
    res.writeHead(200, { 'content-type': 'text/event-stream' });
    for (let i = 0; i < reply.length; i += 6) {
      res.write('data: ' + JSON.stringify({ choices: [{ delta: { content: reply.slice(i, i + 6) } }] }) + '\n\n');
    }
    res.write('data: [DONE]\n\n');
    res.end();
  });
});
server.listen(19999, () => console.log('mock up on 19999'));
