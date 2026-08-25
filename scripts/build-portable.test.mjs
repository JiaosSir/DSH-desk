import assert from 'node:assert/strict'
import { mkdtempSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import test from 'node:test'
import { crc32, listZipEntries } from './zip-entries.mjs'

/** 手工拼一个 store 方法（无压缩）的 zip 字节流。 */
function buildStoreZip(entries) {
  const parts = []
  const central = []
  let offset = 0
  for (const { name, data } of entries) {
    const nameBuf = Buffer.from(name, 'utf8')
    const crc = crc32(data)
    const lh = Buffer.alloc(30)
    lh.writeUInt32LE(0x04034b50, 0)
    lh.writeUInt16LE(20, 4) // version needed
    lh.writeUInt16LE(0, 6) // flags
    lh.writeUInt16LE(0, 8) // method: store
    lh.writeUInt16LE(0, 10) // mod time
    lh.writeUInt16LE(0x21, 12) // mod date
    lh.writeUInt32LE(crc, 14)
    lh.writeUInt32LE(data.length, 18)
    lh.writeUInt32LE(data.length, 22)
    lh.writeUInt16LE(nameBuf.length, 26)
    lh.writeUInt16LE(0, 28) // extra len
    parts.push(lh, nameBuf, data)

    const ch = Buffer.alloc(46)
    ch.writeUInt32LE(0x02014b50, 0)
    ch.writeUInt16LE(20, 4) // version made by
    ch.writeUInt16LE(20, 6) // version needed
    ch.writeUInt16LE(0, 8)
    ch.writeUInt16LE(0, 10) // method
    ch.writeUInt16LE(0, 12)
    ch.writeUInt16LE(0x21, 14)
    ch.writeUInt32LE(crc, 16)
    ch.writeUInt32LE(data.length, 20)
    ch.writeUInt32LE(data.length, 24)
    ch.writeUInt16LE(nameBuf.length, 28)
    ch.writeUInt16LE(0, 30) // extra
    ch.writeUInt16LE(0, 32) // comment
    ch.writeUInt16LE(0, 34) // disk
    ch.writeUInt16LE(0, 36) // internal attrs
    ch.writeUInt32LE(0, 38) // external attrs
    ch.writeUInt32LE(offset, 42)
    central.push(ch, nameBuf)

    offset += lh.length + nameBuf.length + data.length
  }
  const cd = Buffer.concat(central)
  const eocd = Buffer.alloc(22)
  eocd.writeUInt32LE(0x06054b50, 0)
  eocd.writeUInt16LE(entries.length, 8)
  eocd.writeUInt16LE(entries.length, 10)
  eocd.writeUInt32LE(cd.length, 12)
  eocd.writeUInt32LE(offset, 16)
  return Buffer.concat([...parts, cd, eocd])
}

test('listZipEntries 解析 store 方法 zip 的中央目录', async () => {
  const dir = mkdtempSync(join(tmpdir(), 'zip-read-'))
  const zip = join(dir, 't.zip')
  writeFileSync(
    zip,
    buildStoreZip([
      { name: 'DSH-desk-portable-0.1.0-x64/dsh-desk.exe', data: Buffer.from('MZfake') },
      { name: 'DSH-desk-portable-0.1.0-x64/sidecar-dist/node/node.exe', data: Buffer.from('NODE!') },
    ]),
  )
  try {
    const entries = await listZipEntries(zip)
    assert.equal(entries.length, 2)
    assert.deepEqual(entries.map((e) => e.name), [
      'DSH-desk-portable-0.1.0-x64/dsh-desk.exe',
      'DSH-desk-portable-0.1.0-x64/sidecar-dist/node/node.exe',
    ])
    assert.deepEqual(entries.map((e) => e.size), [6, 5])
    assert.ok(entries.every((e) => e.method === 0))
  } finally {
    rmSync(dir, { recursive: true, force: true })
  }
})

test('crc32 与已知向量一致', () => {
  assert.equal(crc32(Buffer.from('123456789')), 0xcbf43926)
})
