/**
 * 最小 zip 中央目录解析器（含 ZIP64）：零依赖列出 zip 内的条目名与大小，
 * 供 build-portable.mjs 校验产物（不依赖外部解压工具，可在受限沙箱跑）。
 */

import { open } from 'node:fs/promises'

const EOCD_SIG = 0x06054b50
const CD_SIG = 0x02014b50
const ZIP64_LOCATOR_SIG = 0x07064b50
const ZIP64_EOCD_SIG = 0x06064b50
const UINT32_MAX = 0xffffffff

/**
 * 在 buffer 中自后向前找 EOCD（容忍 zip 注释）。
 * @returns {number} EOCD 在 buffer 内的偏移，找不到返回 -1。
 */
function findEocd(buf) {
  for (let i = buf.length - 22; i >= 0; i--) {
    if (buf.readUInt32LE(i) === EOCD_SIG) {
      const commentLen = buf.readUInt16LE(i + 20)
      if (i + 22 + commentLen === buf.length) return i
    }
  }
  return -1
}

/** 解析中央目录条目里的 ZIP64 extra（0x0001：解压后大小 8B + 压缩后大小 8B）。 */
function readZip64ExtraSizes(extra, size, compressedSize) {
  let p = 0
  while (p + 4 <= extra.length) {
    const id = extra.readUInt16LE(p)
    const len = extra.readUInt16LE(p + 2)
    if (id === 0x0001 && p + 4 + len <= extra.length) {
      let q = p + 4
      if (size === UINT32_MAX && q + 8 <= extra.length) {
        size = Number(extra.readBigUInt64LE(q))
        q += 8
      }
      if (compressedSize === UINT32_MAX && q + 8 <= extra.length) {
        compressedSize = Number(extra.readBigUInt64LE(q))
      }
      break
    }
    p += 4 + len
  }
  return { size, compressedSize }
}

/**
 * 列出 zip 文件的所有条目（读中央目录，不解压）。
 * @param {string} filePath zip 路径
 * @returns {Promise<Array<{ name: string, size: number, compressedSize: number, method: number }>>}
 */
export async function listZipEntries(filePath) {
  const fh = await open(filePath, 'r')
  try {
    const stat = await fh.stat()
    let tailLen = Math.min(stat.size, 65557)
    let tail = Buffer.alloc(tailLen)
    await fh.read(tail, 0, tailLen, stat.size - tailLen)
    let eocdRel = findEocd(tail)
    if (eocdRel < 0) throw new Error('zip 末尾未找到 EOCD 记录')

    let expected = tail.readUInt16LE(eocdRel + 10)
    let cdSize = tail.readUInt32LE(eocdRel + 12)
    let cdOffset = tail.readUInt32LE(eocdRel + 16)
    const zip64 = expected === 0xffff || cdSize === UINT32_MAX || cdOffset === UINT32_MAX

    if (zip64) {
      // ZIP64 locator 紧贴 EOCD 前 20 字节；若不在当前尾窗内则扩大读取
      if (eocdRel < 20) {
        const baseOld = stat.size - tailLen
        const base = Math.max(0, baseOld - (20 - eocdRel))
        const headroom = baseOld - base
        const newTailLen = stat.size - base
        const buf = Buffer.alloc(newTailLen)
        await fh.read(buf, 0, newTailLen, base)
        tail = buf
        tailLen = newTailLen
        eocdRel += headroom
      }
      const locRel = eocdRel - 20
      if (tail.readUInt32LE(locRel) !== ZIP64_LOCATOR_SIG) {
        throw new Error('zip64 定位器缺失：zip 损坏？')
      }
      const zip64EocdOffset = Number(tail.readBigUInt64LE(locRel + 8))
      const rec = Buffer.alloc(56)
      await fh.read(rec, 0, 56, zip64EocdOffset)
      if (rec.readUInt32LE(0) !== ZIP64_EOCD_SIG) throw new Error('zip64 EOCD 签名不符')
      expected = Number(rec.readBigUInt64LE(32))
      cdSize = Number(rec.readBigUInt64LE(40))
      cdOffset = Number(rec.readBigUInt64LE(48))
    }

    const cd = Buffer.alloc(cdSize)
    await fh.read(cd, 0, cdSize, cdOffset)

    const entries = []
    let p = 0
    while (p + 46 <= cd.length) {
      if (cd.readUInt32LE(p) !== CD_SIG) break
      const method = cd.readUInt16LE(p + 10)
      let compressedSize = cd.readUInt32LE(p + 20)
      let size = cd.readUInt32LE(p + 24)
      const nameLen = cd.readUInt16LE(p + 28)
      const extraLen = cd.readUInt16LE(p + 30)
      const commentLen = cd.readUInt16LE(p + 32)
      const name = cd.toString('utf8', p + 46, p + 46 + nameLen)
      if (zip64 && (size === UINT32_MAX || compressedSize === UINT32_MAX)) {
        const extra = cd.subarray(p + 46 + nameLen, p + 46 + nameLen + extraLen)
        ;({ size, compressedSize } = readZip64ExtraSizes(extra, size, compressedSize))
      }
      entries.push({ name, size, compressedSize, method })
      p += 46 + nameLen + extraLen + commentLen
    }
    if (entries.length !== expected) {
      throw new Error(`中央目录条目数不符：预期 ${expected}，实际 ${entries.length}`)
    }
    return entries
  } finally {
    await fh.close()
  }
}

/** 标准 CRC-32（zip 用），表驱动。 */
export function crc32(buf) {
  let table = crc32.table
  if (!table) {
    table = crc32.table = new Uint32Array(256)
    for (let n = 0; n < 256; n++) {
      let c = n
      for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1
      table[n] = c >>> 0
    }
  }
  let crc = 0xffffffff
  for (let i = 0; i < buf.length; i++) crc = table[(crc ^ buf[i]) & 0xff] ^ (crc >>> 8)
  return (crc ^ 0xffffffff) >>> 0
}
