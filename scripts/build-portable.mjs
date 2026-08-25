#!/usr/bin/env node
/**
 * portable zip 组装（计划任务 5.1 Step 2）：从 `target/release/` 收集
 * `dsh-desk.exe` + 打包资产 `sidecar-dist.tar` / `sidecar-version.json`
 * （Tauri 把 bundle.resources 平铺在 exe 旁边，非 `resources/` 子目录——
 * 以真实构建产物为准），用系统 bsdtar（Windows 10 1803+ / windows-latest
 * 自带 tar.exe）打成 zip，根目录名 `DSH-desk-portable-<version>-x64`，
 * 并校验 zip 内含 exe 与两个资产。方案 A：sidecar 以单个 tar 分发，
 * 应用首启解压到本地缓存（`%LOCALAPPDATA%\com.dsh.desk\sidecar-dist`）。
 *
 * 用法：node scripts/build-portable.mjs [targetDir]
 */

import { spawnSync } from 'node:child_process'
import { cpSync, existsSync, mkdirSync, readFileSync, rmSync, statSync } from 'node:fs'
import { join, resolve } from 'node:path'
import { listZipEntries } from './zip-entries.mjs'

const targetDir = resolve(process.argv[2] ?? 'target/release')
const exe = join(targetDir, 'dsh-desk.exe')
const sidecarTar = join(targetDir, 'sidecar-dist.tar')
const sidecarVersion = join(targetDir, 'sidecar-version.json')

if (!existsSync(exe)) {
  console.error(`portable: exe 不存在: ${exe}（先跑 pnpm --dir apps/desktop tauri build --no-bundle）`)
  process.exit(1)
}
if (!existsSync(sidecarTar) || !existsSync(sidecarVersion)) {
  console.error('portable: 打包资产不完整（sidecar-dist.tar / sidecar-version.json 缺失）——先跑 pnpm sidecar:assemble 再构建')
  process.exit(1)
}

const confPath = resolve('apps/desktop/src-tauri/tauri.conf.json')
const version = JSON.parse(readFileSync(confPath, 'utf8')).version
if (!version) {
  console.error(`portable: ${confPath} 缺 version`)
  process.exit(1)
}

const rootName = `DSH-desk-portable-${version}-x64`
const outDir = resolve('dist-portable')
const staging = join(outDir, rootName)
const zipPath = join(outDir, `${rootName}.zip`)

console.log(`portable: 组装 ${rootName}（版本 ${version}）`)
rmSync(staging, { recursive: true, force: true })
mkdirSync(staging, { recursive: true })

cpSync(exe, join(staging, 'dsh-desk.exe'))
cpSync(sidecarTar, join(staging, 'sidecar-dist.tar'))
cpSync(sidecarVersion, join(staging, 'sidecar-version.json'))

console.log('portable: 打 zip（bsdtar）…')
const tar = join(process.env.SystemRoot ?? 'C:\\Windows', 'System32', 'tar.exe')
if (!existsSync(tar)) {
  console.error('portable: 系统 tar.exe 不存在，无法打 zip')
  process.exit(1)
}
mkdirSync(outDir, { recursive: true })
rmSync(zipPath, { force: true })
const r = spawnSync(tar, ['-a', '-c', '-f', zipPath, '-C', outDir, rootName], {
  stdio: 'inherit',
})
if (r.status !== 0) {
  console.error(`portable: tar 失败 (exit ${r.status})`)
  process.exit(1)
}

// 校验 zip 内容
console.log('portable: 校验 zip 内容…')
const entries = await listZipEntries(zipPath)
const names = new Set(entries.map((e) => e.name))
const want = ['dsh-desk.exe', 'sidecar-dist.tar', 'sidecar-version.json']
for (const w of want) {
  // bsdtar 条目名带根目录前缀，两种形态都接受
  const hit = names.has(w) || names.has(`${rootName}/${w}`)
  if (!hit) {
    console.error(`portable: zip 缺少条目: ${w}`)
    process.exit(1)
  }
}
const total = entries.reduce((acc, e) => acc + e.size, 0)
console.log(`portable: zip 含 ${entries.length} 个条目，解压后约 ${(total / 1024 / 1024).toFixed(0)} MB`)
console.log(`portable: 关键条目校验通过（${want.join('、')}）`)

rmSync(staging, { recursive: true, force: true })
console.log(`portable: OK ${zipPath}（${(statSync(zipPath).size / 1024 / 1024).toFixed(1)} MB）`)
