#!/usr/bin/env node
/**
 * 冒烟测试驱动（计划任务 3.3）：spawn 构建产物 exe，env 注入
 * `DSH_DESK_SMOKE=1`、`DSH_HOME=<mkdtemp>`、`SIDECAR_ROOT=<exe 旁 sidecar-dist>`，
 * 超时 5 分钟，断言 exit 0 且输出含 SMOKE_OK；失败打印 profile 日志尾部。
 */

import { spawn } from 'node:child_process'
import { existsSync, mkdtempSync, readFileSync, readdirSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { dirname, join, resolve } from 'node:path'

const exe = resolve(process.argv[2] ?? 'target/release/dsh-desk.exe')
if (!existsSync(exe)) {
  console.error(`smoke: exe 不存在: ${exe}（先跑 pnpm --dir apps/desktop tauri build --no-bundle）`)
  process.exit(1)
}

const home = mkdtempSync(join(tmpdir(), 'dsh-smoke-'))
const child = spawn(exe, [], {
  env: {
    ...process.env,
    DSH_DESK_SMOKE: '1',
    DSH_HOME: home,
    SIDECAR_ROOT: join(dirname(exe), 'sidecar-dist'),
  },
  stdio: ['ignore', 'pipe', 'pipe'],
})

let out = ''
child.stdout.on('data', (chunk) => { out += chunk })
child.stderr.on('data', (chunk) => { out += chunk })

const timer = setTimeout(() => {
  console.error('smoke: 超时 5 分钟，终止')
  child.kill()
}, 5 * 60 * 1000)

const code = await new Promise((resolveExit) => child.on('exit', resolveExit))
clearTimeout(timer)

const text = out.trim()
console.log(text)

if (code === 0 && text.includes('SMOKE_OK')) {
  console.log('smoke: PASS')
  process.exit(0)
}

console.error(`smoke: FAIL (exit ${code})`)
const logsDir = join(home, 'desktop', 'logs')
if (existsSync(logsDir)) {
  for (const kind of ['sidecar', 'shell']) {
    const files = readdirSync(logsDir).filter((f) => f.startsWith(`${kind}-`)).sort()
    const latest = files.at(-1)
    if (latest) {
      const content = readFileSync(join(logsDir, latest), 'utf8')
      const tail = content.trim().split(/\r?\n/).slice(-30).join('\n')
      console.error(`--- ${kind} 日志尾部（${latest}）---\n${tail}`)
    }
  }
}
process.exit(1)
