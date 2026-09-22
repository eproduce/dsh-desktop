/**
 * 假 Host：按上游 dsh Host 的协议提供最小实现，用于离线验证外壳的启动链路。
 *
 * 它接受与真实 Host 相同的参数位置（argv[2]=runtimeDir、[3]=projectDir、
 * [4]=primaryRuntime），在环回地址上提供一份工作区文档，并按同样的顺序上报
 * `ready` / `shutdown-complete`。
 *
 * 工作区文档把探测到的事实 POST 回 `/report`，可通过 `GET /last-report` 读取。
 * 这样验证不依赖窗口焦点或截图。
 *
 * 用法：node tests/fake-host.mjs <runtimeDir> <projectDir> [primaryRuntime]
 */
import { createServer } from 'node:http'

const [runtimeDir, projectDir, primaryRuntime] = process.argv.slice(2)
if (runtimeDir === undefined || projectDir === undefined) {
  console.error('fake-host: 需要 runtimeDir 与 projectDir')
  process.exit(2)
}

/** 最近一次由工作区文档上报的探测结果，由 `GET /last-report` 读出。 */
let lastReport

/** 工作区文档：运行在 Host 的环回源上，用来验证桥接在非 dsh-app 文档中同样可用。 */
const document = `<!doctype html>
<html lang="zh-CN">
  <head><meta charset="utf-8" /><title>fake workspace</title></head>
  <body style="font: 13px/1.6 -apple-system, sans-serif; padding: 24px">
    <h1 style="font-size:15px;margin:0 0 12px">工作区文档（来自假 Host）</h1>
    <pre id="out">pending</pre>
    <script type="module">
      const out = document.getElementById('out')
      const facts = {
        origin: location.origin,
        platform: document.documentElement.dataset.platform ?? null,
        hasBoot: typeof globalThis.dshDesktopBoot?.ready === 'function',
        hasDesktop: globalThis.dshDesktop?.protocolVersion ?? null,
        hostArgs: ${JSON.stringify([runtimeDir, projectDir, primaryRuntime ?? null])},
      }
      try {
        facts.boot = await globalThis.dshDesktopBoot.ready()
      } catch (error) {
        facts.bootError = String(error)
      }
      out.textContent = JSON.stringify(facts, null, 2)
      await fetch('/report', { method: 'POST', body: JSON.stringify(facts) })
    </script>
  </body>
</html>`

const server = createServer((request, response) => {
  if (request.url === '/last-report') {
    response.writeHead(200, { 'content-type': 'application/json' })
    response.end(JSON.stringify(lastReport ?? null))
    return
  }
  if (request.url === '/report' && request.method === 'POST') {
    let body = ''
    request.on('data', chunk => { body += chunk })
    request.on('end', () => {
      try {
        lastReport = JSON.parse(body)
        console.error(`fake-host: 收到工作区探测结果 ${body}`)
      } catch (error) {
        console.error(`fake-host: 无法解析探测结果：${error.message}`)
      }
      response.writeHead(204)
      response.end()
    })
    return
  }
  response.writeHead(200, { 'content-type': 'text/html; charset=utf-8' })
  response.end(document)
})

// 端口固定便于外部读取探测结果；冲突时回退到随机端口并在日志中说明。
const preferredPort = Number(process.env.DSH_FAKE_HOST_PORT ?? '19387')
const listen = (port, fallback) => {
  server.once('error', error => {
    if (error.code === 'EADDRINUSE' && fallback) {
      console.error(`fake-host: 端口 ${port} 被占用，改用随机端口`)
      listen(0, false)
      return
    }
    process.send?.({ type: 'fatal', message: error.message })
    console.error(error)
  })
  server.listen(port, '127.0.0.1', () => {
    const url = `http://127.0.0.1:${server.address().port}/`
    console.error(`fake-host: 已就绪 ${url}`)
    process.send?.({ type: 'ready', url, injections: [{ marker: 'fake-host' }] })
  })
}
listen(preferredPort, true)

const stop = () => {
  server.close(() => {
    process.send?.({ type: 'shutdown-complete' })
    process.disconnect?.()
  })
}

process.on('message', message => {
  if (message?.type === 'shutdown') stop()
})
process.once('disconnect', stop)
