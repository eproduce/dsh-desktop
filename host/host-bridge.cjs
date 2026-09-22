#!/usr/bin/env node
/**
 * 在 Node IPC 与换行分隔 JSON 之间转发。
 *
 * 上游 dsh Host 用 Node 的 IPC 通道上报（`stdio` 的最后一项为 `ipc`，配合
 * `process.send`）。Rust 进程没有该通道，而在 Windows 上手工传递该 fd 需要处理
 * 句柄继承。因此外壳随包提供这个桥接：它用 IPC 与 Host 通信，把 Host 的消息原样
 * 以换行分隔 JSON 写到自己的 stdout，并把 stdin 上的同样格式转回 Host。
 *
 * Host 的 stdout 与 stderr 都被转发到本进程的 stderr，作为外壳保留的诊断。
 *
 * 用法：node host-bridge.cjs <entry> <runtimeDir> <projectDir> [primaryRuntime]
 */
'use strict'

const { spawn } = require('node:child_process')
const readline = require('node:readline')

const [entry, ...hostArgs] = process.argv.slice(2)
if (entry === undefined) {
  process.stderr.write('host-bridge: 缺少 Host 入口路径\n')
  process.exit(2)
}

const send = message => { process.stdout.write(`${JSON.stringify(message)}\n`) }

const child = spawn(process.execPath, ['--expose-internals', entry, ...hostArgs], {
  cwd: process.env.DSH_HOST_PROJECT ?? process.env.PWD ?? process.cwd(),
  env: process.env,
  stdio: ['ignore', 'pipe', 'pipe', 'ipc'],
})

child.on('message', send)
child.stdout?.on('data', chunk => { process.stderr.write(chunk) })
child.stderr?.on('data', chunk => { process.stderr.write(chunk) })
child.on('error', error => {
  send({ type: 'fatal', message: `host-bridge: 无法启动 Host：${error.message}` })
})

child.on('close', (code, signal) => {
  send({ type: 'exit', code: code ?? -1, signal: signal ?? '' })
  process.exit(code ?? 1)
})

readline.createInterface({ input: process.stdin }).on('line', line => {
  if (line.trim() === '') return
  let message
  try {
    message = JSON.parse(line)
  } catch {
    process.stderr.write('host-bridge: 忽略无法解析的输入\n')
    return
  }
  // 通道关闭代表外壳已停止监管，Host 会据此自行收尾。
  if (child.connected) child.send(message)
})

for (const signal of ['SIGTERM', 'SIGINT']) {
  process.on(signal, () => { child.kill(signal) })
}
