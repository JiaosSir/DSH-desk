/**
 * dsh-desk-bridge 构建配置：宿主半部（lib/index.js，ESM，从 tsc 产物打包，
 * 这样纯类型导入会被擦除）+ 浏览器半部（lib/client.js，闭包工厂产物，通过
 * window.__ModuleLoader__.load 注册——与上游 client bundle 的交接方式一致）。
 *
 * 浏览器 externals 严格等于平台种子模块 + 本包 `dsh.client.inject` 目标
 * （loader 会在本包之前把它们物化进模块表）；其余依赖全部内联。
 * @module tsdown.config
 */

import { defineConfig } from 'tsdown'

/** 插件 id，烙印进 __ModuleLoader__.load 交接调用。 */
const ID = '@cjiaojiao/dsh-desk-bridge'

/** 平台种子模块（上游 packages/client/web/src/platform.ts）。 */
const PLATFORM_MODULES = [
  'react', 'react/jsx-runtime', 'react-dom', 'react-dom/client', '@deepseek-ai/cordis',
  '@deepseek-ai/dsh-client-ui-slots',
  '@deepseek-ai/dsh-client-web-react',
  '@deepseek-ai/dsh-client-ui-primitives',
  '@deepseek-ai/dsh-client-ui-attachment',
  '@deepseek-ai/dsh-client-schema-form',
]

/** 本包 `dsh.client.inject` 目标：先于本包加载，运行时由模块表应答 require。 */
const INJECT_MODULES = [
  '@deepseek-ai/dsh-client-runtime',
  '@deepseek-ai/dsh-client-connection',
  '@deepseek-ai/dsh-client-ui-settings',
]

/** 上游文档化的 runtime 豁免（tsdown.client.ts）。 */
const RUNTIME_STORE_EXEMPTION = '@deepseek-ai/dsh-client-runtime/client'

const CLIENT_EXTERNALS = [...PLATFORM_MODULES, ...INJECT_MODULES, RUNTIME_STORE_EXEMPTION]

export default defineConfig([
  {
    // 宿主半部：把 tsc 产物打包进 lib/index.js（+ invariant）。
    // clean 保持关闭以保留 tsc 生成的 lib/types 声明；
    // fixedExtension 关闭以保住 .js 扩展名（package exports 指向 lib/index.js）。
    name: ID,
    entry: ['lib/types/index.js', 'lib/types/invariant.js'],
    outDir: 'lib',
    format: ['esm'],
    platform: 'node',
    target: 'es2024',
    fixedExtension: false,
    dts: false,
    clean: false,
  },
  {
    // 浏览器半部：经 __ModuleLoader__ 注册的闭包工厂 bundle。
    name: `${ID}/client`,
    entry: { client: 'src/client/index.ts' },
    outDir: 'lib',
    format: 'cjs',
    platform: 'browser',
    dts: false,
    sourcemap: true,
    clean: false,
    external: CLIENT_EXTERNALS,
    // 模块表应答不了的 require 必抛运行时异常，所以表外依赖一律内联。
    noExternal: (id: string) => (CLIENT_EXTERNALS.includes(id) ? undefined : true),
    define: {
      'process.env.NODE_ENV': JSON.stringify(process.env.NODE_ENV ?? 'production'),
      'import.meta.env.MODE': JSON.stringify(process.env.NODE_ENV ?? 'production'),
      'import.meta.env': JSON.stringify({ MODE: process.env.NODE_ENV ?? 'production' }),
    },
    outputOptions: {
      entryFileNames: 'client.js',
      banner: `window.__ModuleLoader__.load({ id: ${JSON.stringify(ID)}, factory: (require) => {`,
      footer: 'return module.exports; } });',
      intro: 'var module = { exports: {} }; var exports = module.exports;',
    },
  },
])
