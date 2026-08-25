#!/usr/bin/env node
/**
 * sidecar 组装脚本：可复现地产出 apps/desktop/src-tauri/sidecar-dist/。
 *
 * 产物构成（全部钉死版本）：
 * - node/            Node 运行时（官方 win-x64 zip 解包）
 * - pnpm/            pnpm standalone（pnpm.cjs）+ pnpm.cmd shim（指向自带 node）
 * - node_modules/    @deepseek-ai/dsh 钉版依赖树（自带 node+pnpm 安装）
 * - VERSION.json     三版本 + 组装时间（幂等判定依据）
 *
 * 幂等：VERSION.json 三版本与常量一致时跳过全部下载与安装；--force 强制重建。
 *
 * 用法：
 *   node scripts/assemble-sidecar.mjs            # 幂等组装
 *   node scripts/assemble-sidecar.mjs --force    # 强制重建
 */

import {
  copyFileSync,
  createWriteStream,
  existsSync,
  mkdirSync,
  readFileSync,
  readdirSync,
  renameSync,
  rmSync,
  statSync,
  writeFileSync,
} from 'node:fs'
import { spawn, spawnSync } from 'node:child_process'
import { dirname, join, relative } from 'node:path'
import { fileURLToPath } from 'node:url'

// ── 钉版常量（实现期统一升级点，与计划 D10 对齐） ─────────────────────────
export const NODE_VERSION = '24.19.0' // Node 24 LTS（harness engines >=24.0.0）
export const PNPM_VERSION = '11.7.0' // 与本机验证环境一致（pnpm ≥10 满足 profile 语义）
export const DSH_VERSION = '0.1.1-rc.2' // npm 最新 rc（latest/next 标签；@deepseek-ai/dsh，2026-08）

const REPO_ROOT = fileURLToPath(new URL('..', import.meta.url))
export const SIDECAR_DIR = join(REPO_ROOT, 'apps', 'desktop', 'src-tauri', 'sidecar-dist')
/// 下载缓存目录：命中即免下载；手动下载的文件也可以直接放进这里。
export const DOWNLOAD_CACHE = join(REPO_ROOT, '.downloads')
const NODE_ZIP = `node-v${NODE_VERSION}-win-x64.zip`
const NODE_ZIP_URL = `https://nodejs.org/dist/v${NODE_VERSION}/${NODE_ZIP}`
const PNPM_TGZ_URL = `https://registry.npmjs.org/pnpm/-/pnpm-${PNPM_VERSION}.tgz`

// ── 纯函数（可测） ─────────────────────────────────────────────────────────

/** pnpm.cmd shim 内容：经自带 node 运行 bin/pnpm.cjs。必须是 CRLF（F5 的 .cmd 语义）。 */
export function buildPnpmShim() {
  return '@"%~dp0..\\node\\node.exe" "%~dp0bin\\pnpm.cjs" %*\r\n'
}

/** 版本常量合法性检查（DSH 形如 0.1.0-rc.N）。 */
export function validateVersions() {
  const problems = []
  if (!/^\d+\.\d+\.\d+$/.test(NODE_VERSION)) problems.push(`NODE_VERSION 非法: ${NODE_VERSION}`)
  if (!/^\d+\.\d+\.\d+$/.test(PNPM_VERSION)) problems.push(`PNPM_VERSION 非法: ${PNPM_VERSION}`)
  if (!/^\d+\.\d+\.\d+-rc\.\d+$/.test(DSH_VERSION)) problems.push(`DSH_VERSION 非法: ${DSH_VERSION}`)
  return problems
}

/**
 * sidecar 的 pnpm-workspace.yaml：`nodeLinker: hoisted` 是打包完整性硬要求——
 * Tauri 打包 bundle.resources 时会跳过符号链接（isolated linker 的
 * node_modules/@scope 全是 symlink），必须用扁平真实目录才能完整进入安装包。
 */
export function buildWorkspaceYaml() {
  return [
    'packages:',
    '  - .',
    '',
    'nodeLinker: hoisted',
    '',
    'allowBuilds:',
    "  '@deepseek-ai/dsh-subprocess-local': true",
    "  '@google/genai': true",
    '  koffi: true',
    '  node-pty: true',
    '  protobufjs: true',
    '',
  ].join('\n')
}

/** 已组装的 sidecar 是否与当前钉版一致（幂等判定）。 */
export function isCurrent(dir = SIDECAR_DIR) {
  const versionFile = join(dir, 'VERSION.json')
  if (!existsSync(versionFile)) return false
  try {
    const v = JSON.parse(readFileSync(versionFile, 'utf8'))
    return v.node === NODE_VERSION && v.pnpm === PNPM_VERSION && v.dsh === DSH_VERSION
  } catch {
    return false
  }
}

// ── 下载与解包 ─────────────────────────────────────────────────────────────

function sleep(ms) {
  return new Promise((resolve) => setTimeout(resolve, ms))
}

/** 流式下载（可注入替换以便测试）。默认实现带简单重试。 */
export async function downloadFile(url, dest, retries = 3) {
  for (let attempt = 1; ; attempt += 1) {
    try {
      const response = await fetch(url)
      if (!response.ok || response.body === null) {
        throw new Error(`HTTP ${response.status}: ${url}`)
      }
      const total = Number(response.headers.get('content-length') ?? '0')
      let got = 0
      const file = createWriteStream(dest)
      const reader = response.body.getReader()
      for (;;) {
        const { done, value } = await reader.read()
        if (done) break
        got += value.length
        if (!file.write(Buffer.from(value))) {
          await new Promise((resolve) => file.once('drain', resolve))
        }
        if (attempt === 1 && total > 0 && got % (5 * 1024 * 1024) < 4096) {
          console.log(`  ${dest.split(/[\\/]/).pop()}: ${(got / 1024 / 1024).toFixed(1)} / ${(total / 1024 / 1024).toFixed(1)} MB`)
        }
      }
      file.end()
      await new Promise((resolve, reject) => {
        file.on('finish', resolve)
        file.on('error', reject)
      })
      return dest
    } catch (error) {
      if (attempt >= retries) throw error
      console.warn(`  下载失败（第 ${attempt} 次）: ${error.message}，重试…`)
      await sleep(2000 * attempt)
    }
  }
}

/** 解包 zip/tgz（Windows 自带 bsdtar 支持两种格式）。 */
function extractArchive(archivePath, destDir) {
  mkdirSync(destDir, { recursive: true })
  const result = spawnSync('tar', ['-xf', archivePath, '-C', destDir], { stdio: 'inherit' })
  if (result.status !== 0) {
    throw new Error(`tar 解包失败（退出码 ${result.status}）: ${archivePath}`)
  }
}

/** 目录里递归找第一个 basename 匹配的文件（pnpm tarball 内路径随版本变化）。 */
function findFile(dir, basename) {
  const entries = readdirSync(dir, { withFileTypes: true })
  for (const entry of entries) {
    const full = join(dir, entry.name)
    if (entry.isFile() && entry.name === basename) return full
    if (entry.isDirectory()) {
      const found = findFile(full, basename)
      if (found !== null) return found
    }
  }
  return null
}

/**
 * 探测 VS 自带的 CMake（koffi 等原生模块源码编译需要）。
 * CI（windows-latest）的 cmake 已在 PATH 上，本函数只服务本机开发。
 * @returns cmake.exe 所在目录；未找到返回 null。
 */
function findVsCmakeDir() {
  const roots = [
    'C:\\Program Files (x86)\\Microsoft Visual Studio',
    'C:\\Program Files\\Microsoft Visual Studio',
  ]
  for (const root of roots) {
    if (!existsSync(root)) continue
    // 布局：<root>/<edition 如 2019>/<instance 如 Community>/Common7/…
    for (const edition of readdirSync(root)) {
      const editionDir = join(root, edition)
      if (!statSync(editionDir).isDirectory()) continue
      for (const instance of readdirSync(editionDir)) {
        const candidate = join(
          editionDir,
          instance,
          'Common7',
          'IDE',
          'CommonExtensions',
          'Microsoft',
          'CMake',
          'CMake',
          'bin',
        )
        if (existsSync(join(candidate, 'cmake.exe'))) return candidate
      }
    }
  }
  return null
}

/** 递归统计目录字节数（内部：单位必须是字节，避免与 MB 混加）。 */
function dirSizeBytes(dir) {
  let total = 0
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    const full = join(dir, entry.name)
    if (entry.isDirectory()) total += dirSizeBytes(full)
    else total += statSync(full).size
  }
  return total
}

/** 目录体积（MB，供 README 记录）。 */
export function dirSizeMb(dir) {
  return dirSizeBytes(dir) / 1024 / 1024
}

/** 带本地缓存的下载：先查 .downloads/，命中则直接复制；否则下载并写回缓存。 */
async function downloadCached(url, dest) {
  const name = url.split('/').pop()
  const cached = join(DOWNLOAD_CACHE, name)
  if (existsSync(cached) && statSync(cached).size > 0) {
    copyFileSync(cached, dest)
    console.log(`  命中缓存: ${name}`)
    return dest
  }
  await downloadFile(url, dest)
  mkdirSync(DOWNLOAD_CACHE, { recursive: true })
  copyFileSync(dest, cached)
  console.log(`  已缓存: ${name}`)
  return dest
}

// ── 组装主流程 ─────────────────────────────────────────────────────────────

/**
 * 组装 sidecar（可注入下载/解包/安装函数以便测试桩替换）。
 * @param {object} opts
 * @param {Function} [opts.downloadFile] 下载函数 (url, dest) => Promise<dest>
 * @param {Function} [opts.extractArchive] 解包函数 (archivePath, destDir) => void
 * @param {Function} [opts.runPnpmInstall] 安装函数 (sidecarDir) => Promise<void>
 * @param {string} [opts.sidecarDir] 产物目录（默认 SIDECAR_DIR）
 */
export async function assemble(opts = {}) {
  const {
    downloadFile: download = downloadCached,
    extractArchive: extract = extractArchive,
    runPnpmInstall,
    sidecarDir = SIDECAR_DIR,
  } = opts
  const nodeDir = join(sidecarDir, 'node')
  const pnpmDir = join(sidecarDir, 'pnpm')
  const tmpDir = join(sidecarDir, '.tmp')

  rmSync(sidecarDir, { recursive: true, force: true })
  mkdirSync(tmpDir, { recursive: true })

  // 1. Node 运行时
  console.log(`[1/4] 下载 Node v${NODE_VERSION}…`)
  const nodeZip = join(tmpDir, NODE_ZIP)
  await download(NODE_ZIP_URL, nodeZip)
  mkdirSync(nodeDir, { recursive: true })
  extract(nodeZip, nodeDir)
  // zip 内有一层目录 node-vX-win-x64/，摊平到 node/ 下。
  const inner = join(nodeDir, `node-v${NODE_VERSION}-win-x64`)
  if (existsSync(inner)) {
    for (const entry of readdirSync(inner)) {
      const from = join(inner, entry)
      const to = join(nodeDir, entry)
      rmSync(to, { recursive: true, force: true })
      renameSync(from, to)
    }
    rmSync(inner, { recursive: true, force: true })
  }
  if (!existsSync(join(nodeDir, 'node.exe'))) {
    throw new Error(`node.exe 未在预期位置: ${nodeDir}`)
  }
  console.log('  node.exe 就绪')

  // 2. pnpm standalone + .cmd shim
  console.log(`[2/4] 下载 pnpm v${PNPM_VERSION}…`)
  const pnpmTgz = join(tmpDir, `pnpm-${PNPM_VERSION}.tgz`)
  await download(PNPM_TGZ_URL, pnpmTgz)
  const extractDir = join(tmpDir, 'pnpm-extract')
  extract(pnpmTgz, extractDir)
  const pnpmCjs = findFile(extractDir, 'pnpm.cjs')
  if (pnpmCjs === null) {
    throw new Error('pnpm tarball 内未找到 pnpm.cjs')
  }
  // pnpm standalone 布局：<package>/bin/pnpm.cjs（入口）+ <package>/dist/（实现与
  // 全部内嵌依赖）。bin 与 dist 的相对关系必须保持，所以把整个包根目录搬进
  // pnpmDir，不做任何挑拣。
  const rel = relative(extractDir, pnpmCjs)
  const packageRoot = join(extractDir, rel.split(/[\\/]/)[0])
  mkdirSync(pnpmDir, { recursive: true })
  for (const entry of readdirSync(packageRoot)) {
    renameSync(join(packageRoot, entry), join(pnpmDir, entry))
  }
  writeFileSync(join(pnpmDir, 'pnpm.cmd'), buildPnpmShim(), 'utf8')
  console.log('  pnpm（bin/ + dist/）与 pnpm.cmd 就绪')

  // 3. 钉版依赖树（自带 node+pnpm，PATH 注入验证 F5 的 .cmd shim 路径）
  console.log(`[3/4] 安装 @deepseek-ai/dsh@${DSH_VERSION}（可能较久）…`)
  writeFileSync(
    join(sidecarDir, 'package.json'),
    JSON.stringify({ private: true, dependencies: { '@deepseek-ai/dsh': DSH_VERSION } }, null, 2),
  )
  // pnpm ≥10 的构建脚本白名单 + nodeLinker: hoisted 都写在 pnpm-workspace.yaml
  // （见 buildWorkspaceYaml 注释：hoisted 是打包完整性硬要求）。
  writeFileSync(join(sidecarDir, 'pnpm-workspace.yaml'), buildWorkspaceYaml())
  const nodeExe = join(nodeDir, 'node.exe')
  const pnpmCjsPath = join(pnpmDir, 'bin', 'pnpm.cjs')
  const env = {
    ...process.env,
    // pnpm 会向上找到仓库根的 pnpm-workspace.yaml 并误把 sidecar 目录
    // 当成员；CI=true 同时跳过"是否清空模块目录"的 TTY 确认。
    CI: 'true',
    PATH: [pnpmDir, findVsCmakeDir(), process.env.PATH ?? '']
      .filter(Boolean)
      .join(process.platform === 'win32' ? ';' : ':'),
  }
  const runInstall = () =>
    new Promise((resolve) => {
      // 真实安装：node bin/pnpm.cjs install；输出走 inherit（无管道 stdio）。
      // 注意：不能加 --no-optional——sharp 的 win32 二进制经 optionalDependencies
      // 分发，排除后宿主启动即 fail-loud。
      const result = spawn(nodeExe, [pnpmCjsPath, 'install', '--prod'], {
        cwd: sidecarDir,
        stdio: 'inherit',
        env,
      })
      result.on('exit', resolve)
    })
  if (runPnpmInstall !== undefined) {
    await runPnpmInstall(sidecarDir)
  } else {
    const code = await runInstall()
    if (code !== 0) {
      throw new Error(`pnpm install 退出码 ${code}`)
    }
  }
  if (!existsSync(join(sidecarDir, 'node_modules', '@deepseek-ai', 'dsh', 'package.json'))) {
    throw new Error('安装后未找到 @deepseek-ai/dsh')
  }
  console.log('  依赖树就绪')

  // 4. 收尾：清理临时目录，落 VERSION.json；package.json 去掉依赖声明（node_modules 保留）。
  rmSync(tmpDir, { recursive: true, force: true })
  writeFileSync(
    join(sidecarDir, 'package.json'),
    JSON.stringify({ private: true, name: 'dsh-desk-sidecar' }, null, 2) + '\n',
  )
  writeFileSync(
    join(sidecarDir, 'VERSION.json'),
    JSON.stringify(
      {
        node: NODE_VERSION,
        pnpm: PNPM_VERSION,
        dsh: DSH_VERSION,
        assembledAt: new Date().toISOString(),
      },
      null,
      2,
    ) + '\n',
  )
  console.log('[4/4] 完成')
  return sidecarDir
}

// ── CLI 入口 ───────────────────────────────────────────────────────────────

const isMain = process.argv[1] !== undefined
  && fileURLToPath(import.meta.url) === fileURLToPath(new URL(`file://${process.argv[1].replaceAll('\\', '/')}`))

if (isMain) {
  const problems = validateVersions()
  if (problems.length > 0) {
    for (const problem of problems) console.error(`版本常量错误: ${problem}`)
    process.exit(1)
  }
  const force = process.argv.includes('--force')
  if (!force && isCurrent()) {
    console.log(`sidecar 已是最新（node ${NODE_VERSION} / pnpm ${PNPM_VERSION} / dsh ${DSH_VERSION}），跳过。`)
    console.log('如需重建: node scripts/assemble-sidecar.mjs --force')
    process.exit(0)
  }
  assemble()
    .then((dir) => {
      console.log(`sidecar 组装完成: ${dir}`)
      console.log(`安装体积: ${dirSizeMb(dir).toFixed(0)} MB`)
    })
    .catch((error) => {
      console.error(`组装失败: ${error.message}`)
      process.exit(1)
    })
}
