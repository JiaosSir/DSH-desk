/**
 * assemble-sidecar.mjs 的单元测试（node:test，全部离线）：
 * - 纯函数：shim 内容（CRLF）、版本常量、幂等判定；
 * - assemble() 全流程：注入桩下载/解包/安装，断言产物树结构与 VERSION.json。
 */

import test from 'node:test'
import assert from 'node:assert/strict'
import {
  existsSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'

import {
  assemble,
  buildPnpmShim,
  buildWorkspaceYaml,
  DSH_VERSION,
  isCurrent,
  NODE_VERSION,
  PNPM_VERSION,
  validateVersions,
} from './assemble-sidecar.mjs'

test('workspace yaml 要求 hoisted 与 allowBuilds 白名单', () => {
  const yaml = buildWorkspaceYaml()
  assert.ok(yaml.includes('nodeLinker: hoisted'), '必须 hoisted（Tauri 打包跳过 symlink）')
  assert.ok(yaml.includes("'@deepseek-ai/dsh-subprocess-local': true"))
  assert.ok(yaml.includes('koffi: true'))
  assert.ok(yaml.includes('node-pty: true'))
})

test('pnpm.cmd shim 内容精确且为 CRLF', () => {
  const shim = buildPnpmShim()
  assert.equal(shim, '@"%~dp0..\\node\\node.exe" "%~dp0bin\\pnpm.cjs" %*\r\n')
  assert.ok(shim.endsWith('\r\n'), '必须以 CRLF 结尾')
  assert.ok(!/[^\r]\n/.test(shim), '不得出现裸 LF')
})

test('钉版常量格式合法', () => {
  assert.deepEqual(validateVersions(), [])
  assert.match(NODE_VERSION, /^\d+\.\d+\.\d+$/)
  assert.match(PNPM_VERSION, /^\d+\.\d+\.\d+$/)
  assert.match(DSH_VERSION, /^\d+\.\d+\.\d+-rc\.\d+$/)
})

test('isCurrent 幂等判定', () => {
  const dir = mkdtempSync(join(tmpdir(), 'dsh-sidecar-'))
  try {
    assert.equal(isCurrent(dir), false, '无 VERSION.json 时不是最新')
    writeFileSync(
      join(dir, 'VERSION.json'),
      JSON.stringify({ node: NODE_VERSION, pnpm: PNPM_VERSION, dsh: DSH_VERSION }),
    )
    assert.equal(isCurrent(dir), true, '三版本一致时是最新')
    writeFileSync(
      join(dir, 'VERSION.json'),
      JSON.stringify({ node: NODE_VERSION, pnpm: '9.9.9', dsh: DSH_VERSION }),
    )
    assert.equal(isCurrent(dir), false, '版本漂移时不是最新')
    writeFileSync(join(dir, 'VERSION.json'), '{ 坏掉的 json')
    assert.equal(isCurrent(dir), false, '损坏的 VERSION.json 视为非最新')
  } finally {
    rmSync(dir, { recursive: true, force: true })
  }
})

test('assemble 全流程产物结构正确', async () => {
  const dir = mkdtempSync(join(tmpdir(), 'dsh-sidecar-'))
  try {
    const calls = []
    const fakeZipPath = join(dir, 'fake.zip')
    const fakeTgzPath = join(dir, 'fake.tgz')
    writeFileSync(fakeZipPath, 'fake-zip')
    writeFileSync(fakeTgzPath, 'fake-tgz')

    await assemble({
      sidecarDir: join(dir, 'sidecar-dist'),
      downloadFile: async (url, dest) => {
        calls.push(url)
        writeFileSync(dest, `downloaded:${url}`)
        return dest
      },
      extractArchive: (archivePath, destDir) => {
        calls.push(archivePath)
        mkdirSync(destDir, { recursive: true })
        if (archivePath.endsWith('.zip')) {
          // 模拟 node zip：内层目录 + node.exe
          mkdirSync(join(destDir, `node-v${NODE_VERSION}-win-x64`), { recursive: true })
          writeFileSync(join(destDir, `node-v${NODE_VERSION}-win-x64`, 'node.exe'), 'exe')
        } else {
          // 模拟 pnpm standalone 布局：package/bin/pnpm.cjs（入口）+ package/dist/pnpm.mjs
          mkdirSync(join(destDir, 'package', 'bin'), { recursive: true })
          mkdirSync(join(destDir, 'package', 'dist'), { recursive: true })
          writeFileSync(join(destDir, 'package', 'bin', 'pnpm.cjs'), 'pnpm-entry')
          writeFileSync(join(destDir, 'package', 'dist', 'pnpm.mjs'), 'pnpm-impl')
        }
      },
      runPnpmInstall: async (sidecarDir) => {
        calls.push(`install:${sidecarDir}`)
        mkdirSync(join(sidecarDir, 'node_modules', '@deepseek-ai', 'dsh'), { recursive: true })
        writeFileSync(join(sidecarDir, 'node_modules', '@deepseek-ai', 'dsh', 'package.json'), '{}')
      },
    })

    const out = join(dir, 'sidecar-dist')
    // 产物树
    assert.ok(existsSync(join(out, 'node', 'node.exe')), 'node/node.exe 应存在')
    assert.ok(existsSync(join(out, 'pnpm', 'bin', 'pnpm.cjs')), 'pnpm/bin/pnpm.cjs 应存在')
    assert.ok(existsSync(join(out, 'pnpm', 'dist', 'pnpm.mjs')), 'pnpm/dist/pnpm.mjs 应存在')
    assert.ok(existsSync(join(out, 'pnpm', 'pnpm.cmd')), 'pnpm/pnpm.cmd 应存在')
    assert.ok(
      existsSync(join(out, 'node_modules', '@deepseek-ai', 'dsh', 'package.json')),
      'dsh 依赖树应存在',
    )
    // zip 内层目录应被摊平（不存在嵌套 node-vX-win-x64 目录）
    assert.equal(existsSync(join(out, 'node', `node-v${NODE_VERSION}-win-x64`)), false)
    // shim 内容
    assert.equal(readFileSync(join(out, 'pnpm', 'pnpm.cmd'), 'utf8'), buildPnpmShim())
    // VERSION.json
    const version = JSON.parse(readFileSync(join(out, 'VERSION.json'), 'utf8'))
    assert.equal(version.node, NODE_VERSION)
    assert.equal(version.pnpm, PNPM_VERSION)
    assert.equal(version.dsh, DSH_VERSION)
    assert.ok(typeof version.assembledAt === 'string')
    // package.json 收尾后不再含依赖声明
    const pkg = JSON.parse(readFileSync(join(out, 'package.json'), 'utf8'))
    assert.equal(pkg.dependencies, undefined)
    // 下载了 node zip 与 pnpm tarball 各一次，且安装被调用
    assert.ok(calls.some((c) => c.includes('nodejs.org/dist')), '应下载 node zip')
    assert.ok(calls.some((c) => c.includes('registry.npmjs.org/pnpm')), '应下载 pnpm tarball')
    assert.ok(calls.some((c) => c.startsWith('install:')), '应执行安装')
  } finally {
    rmSync(dir, { recursive: true, force: true })
  }
})
