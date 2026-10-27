// VideoPsalm `.vpc` / `.vpcsetting` container support.
//
// VideoPsalm's `VideoPsalmJSon.FromFile` treats these extensions as Ionic.Zip
// archives: it extracts entry 0 with `Extension.GetFileName()` as the password.
// `ToFile(..., isCompressed: true, isProtected: true)` writes one entry whose
// name is the songbook filename with a `.json` suffix. Protected entries use
// WinZip AES-256, which is what this module reads and writes.
//
// The password below is not a user secret. It is the ASCII result of
// `Extension.GetFileName()` in VideoPsalm 1.29 (a fixed encrypted byte array
// decrypted with Rijndael); see decompiled `VideoPsalm/Extension.cs:1134`.
// Keeping it here is what makes a real `.vpc` editable without asking the
// operator for a password they never set.

import * as fs from "fs";
import {
  createCipheriv,
  createHmac,
  pbkdf2Sync,
  randomBytes,
  timingSafeEqual,
} from "crypto";
import { deflateRawSync, inflateRawSync } from "zlib";

export const VIDEOPSALM_VPC_PASSWORD =
  "5$l(va\\7N_0!',/:5?M0.(d_<8+5!Um2#~M@{5#2(C3]$'f|#:Y/}wG+-)Nt:x^8!Y.5Eq$)'7stk$y)\\.~Tg=&<l";

const AES_EXTRA_ID = 0x9901;
const LOCAL_SIG = 0x04034b50;
const CENTRAL_SIG = 0x02014b50;
const EOCD_SIG = 0x06054b50;

export interface VpcEntry {
  name: string;
  data: Buffer;
  encrypted: boolean;
}

export interface VpcWriteOptions {
  /** Defaults to VideoPsalm's fixed container password. */
  password?: string;
  /** Preserve an unprotected archive as unprotected; defaults to true. */
  encrypt?: boolean;
}

interface AesInfo {
  strength: number;
  actualMethod: number;
}

interface CentralEntry {
  name: string;
  flags: number;
  method: number;
  compressedSize: number;
  uncompressedSize: number;
  localOffset: number;
  extra: Buffer;
}

function parseAesExtra(extra: Buffer): AesInfo | null {
  for (let i = 0; i + 4 <= extra.length; ) {
    const id = extra.readUInt16LE(i);
    const len = extra.readUInt16LE(i + 2);
    const end = i + 4 + len;
    if (end > extra.length) return null;
    if (id === AES_EXTRA_ID && len >= 7) {
      return { strength: extra[i + 8], actualMethod: extra.readUInt16LE(i + 9) };
    }
    i = end;
  }
  return null;
}

function findEocd(buf: Buffer): number {
  const start = Math.max(0, buf.length - 22 - 0xffff);
  for (let i = buf.length - 22; i >= start; i--) {
    if (buf.readUInt32LE(i) === EOCD_SIG) return i;
  }
  throw new Error("Not a ZIP archive (end-of-central-directory record not found)");
}

function centralEntries(buf: Buffer): CentralEntry[] {
  const eocd = findEocd(buf);
  const count = buf.readUInt16LE(eocd + 10);
  const offset = buf.readUInt32LE(eocd + 16);
  if (count === 0xffff || offset === 0xffffffff) {
    throw new Error("ZIP64 .vpc archives are not supported");
  }
  const entries: CentralEntry[] = [];
  let p = offset;
  for (let i = 0; i < count; i++) {
    if (p + 46 > buf.length || buf.readUInt32LE(p) !== CENTRAL_SIG) {
      throw new Error("Malformed ZIP central directory");
    }
    const flags = buf.readUInt16LE(p + 8);
    const method = buf.readUInt16LE(p + 10);
    const compressedSize = buf.readUInt32LE(p + 20);
    const uncompressedSize = buf.readUInt32LE(p + 24);
    const nameLen = buf.readUInt16LE(p + 28);
    const extraLen = buf.readUInt16LE(p + 30);
    const commentLen = buf.readUInt16LE(p + 32);
    const localOffset = buf.readUInt32LE(p + 42);
    const nameStart = p + 46;
    if (nameStart + nameLen + extraLen + commentLen > buf.length) {
      throw new Error("Malformed ZIP central-directory entry");
    }
    const name = buf.subarray(nameStart, nameStart + nameLen).toString(flags & 0x800 ? "utf8" : "latin1");
    const extra = buf.subarray(nameStart + nameLen, nameStart + nameLen + extraLen);
    entries.push({ name, flags, method, compressedSize, uncompressedSize, localOffset, extra });
    p = nameStart + nameLen + extraLen + commentLen;
  }
  return entries;
}

/** AES-CTR with WinZip's 128-bit little-endian block counter (starts at 1). */
function aesCtrCrypt(key: Buffer, data: Buffer): Buffer {
  const cipher = createCipheriv(`aes-${key.length * 8}-ecb`, key, null);
  cipher.setAutoPadding(false);
  const counter = Buffer.alloc(16);
  counter.writeUIntLE(1, 0, 6);
  const out = Buffer.alloc(data.length);
  for (let p = 0; p < data.length; p += 16) {
    const stream = cipher.update(counter);
    const n = Math.min(16, data.length - p);
    for (let i = 0; i < n; i++) out[p + i] = data[p + i] ^ stream[i];
    for (let i = 0; i < 16; i++) {
      counter[i] = (counter[i] + 1) & 0xff;
      if (counter[i] !== 0) break;
    }
  }
  return out;
}

function extractAes(data: Buffer, aes: AesInfo, uncompressedSize: number, password: string): Buffer {
  const saltLen = aes.strength === 1 ? 8 : aes.strength === 2 ? 12 : 16;
  const keyLen = aes.strength === 1 ? 16 : aes.strength === 2 ? 24 : 32;
  if (data.length < saltLen + 2 + 10) throw new Error("Truncated WinZip AES entry");
  const salt = data.subarray(0, saltLen);
  const verifier = data.subarray(saltLen, saltLen + 2);
  const ciphertext = data.subarray(saltLen + 2, data.length - 10);
  const auth = data.subarray(data.length - 10);
  const derived = pbkdf2Sync(password, salt, 1000, keyLen * 2 + 2, "sha1");
  const key = derived.subarray(0, keyLen);
  const authKey = derived.subarray(keyLen, keyLen * 2);
  const expected = derived.subarray(keyLen * 2);
  if (!timingSafeEqual(verifier, expected)) {
    throw new Error("Incorrect .vpc password (or the archive is not WinZip-AES)");
  }
  const hmac = createHmac("sha1", authKey).update(ciphertext).digest().subarray(0, 10);
  if (!timingSafeEqual(auth, hmac)) throw new Error("Corrupt .vpc archive (authentication failed)");
  const plain = aesCtrCrypt(key, ciphertext);
  return decompress(plain, aes.actualMethod, uncompressedSize);
}

function decompress(data: Buffer, method: number, uncompressedSize: number): Buffer {
  let out: Buffer;
  if (method === 0) out = data;
  else if (method === 8) out = inflateRawSync(data);
  else throw new Error(`Unsupported .vpc compression method ${method}`);
  if (uncompressedSize !== 0xffffffff && out.length !== uncompressedSize) {
    throw new Error("Corrupt .vpc archive (uncompressed size mismatch)");
  }
  return out;
}

/**
 * Read every file entry of a VideoPsalm ZIP container.
 *
 * A `.vpc` songbook holds one entry; a `.vpagd` service plan holds many
 * (`Song_0.json`, `SongBook_0.json`, `BibleVerses_0.json`, the media blobs…).
 * The agenda archives are not encrypted; the protected songbooks are
 * (WinZip AES-256 with VideoPsalm's fixed password).
 */
export function readVpcEntries(
  file: string,
  password = VIDEOPSALM_VPC_PASSWORD
): VpcEntry[] {
  const buf = fs.readFileSync(file);
  const out: VpcEntry[] = [];
  for (const entry of centralEntries(buf)) {
    if (entry.name.endsWith("/")) continue;
    const local = entry.localOffset;
    if (local + 30 > buf.length || buf.readUInt32LE(local) !== LOCAL_SIG) {
      throw new Error("Malformed .vpc local file header");
    }
    const localFlags = buf.readUInt16LE(local + 6);
    const localMethod = buf.readUInt16LE(local + 8);
    const nameLen = buf.readUInt16LE(local + 26);
    const extraLen = buf.readUInt16LE(local + 28);
    const dataStart = local + 30 + nameLen + extraLen;
    const localExtra = buf.subarray(local + 30 + nameLen, dataStart);
    const flags = entry.flags || localFlags;
    const method = entry.method || localMethod;
    const compressedSize = entry.compressedSize;
    if (dataStart + compressedSize > buf.length) throw new Error("Truncated .vpc entry data");
    const payload = buf.subarray(dataStart, dataStart + compressedSize);
    const encrypted = (flags & 1) !== 0;
    if (!encrypted) {
      out.push({
        name: entry.name,
        data: decompress(payload, method, entry.uncompressedSize),
        encrypted: false,
      });
      continue;
    }
    if (method !== 99) {
      throw new Error("This .vpc uses legacy ZipCrypto encryption, which VideoPsalm 1.29 does not write");
    }
    const aes = parseAesExtra(localExtra) ?? parseAesExtra(entry.extra);
    if (!aes) throw new Error("Encrypted .vpc is missing its WinZip-AES metadata");
    out.push({
      name: entry.name,
      data: extractAes(payload, aes, entry.uncompressedSize, password),
      encrypted: true,
    });
  }
  return out;
}

/** Read the first file entry from a VideoPsalm `.vpc` archive. */
export function readVpcEntry(
  file: string,
  password = VIDEOPSALM_VPC_PASSWORD
): VpcEntry {
  const entry = readVpcEntries(file, password)[0];
  if (!entry) throw new Error("The .vpc archive contains no file entry");
  return entry;
}

function crc32(data: Buffer): number {
  let crc = 0xffffffff;
  for (const byte of data) {
    crc ^= byte;
    for (let i = 0; i < 8; i++) crc = (crc >>> 1) ^ (0xedb88320 & -(crc & 1));
  }
  return (crc ^ 0xffffffff) >>> 0;
}

function dosDateTime(d: Date): { time: number; date: number } {
  const time = (d.getHours() << 11) | (d.getMinutes() << 5) | Math.floor(d.getSeconds() / 2);
  const date = ((d.getFullYear() - 1980) << 9) | ((d.getMonth() + 1) << 5) | d.getDate();
  return { time, date };
}

function aesExtra(): Buffer {
  const extra = Buffer.alloc(11);
  extra.writeUInt16LE(AES_EXTRA_ID, 0);
  extra.writeUInt16LE(7, 2);
  extra.writeUInt16LE(2, 4); // AE-2
  extra.write("AE", 6, "ascii");
  extra[8] = 3; // AES-256
  extra.writeUInt16LE(8, 9); // actual method: deflate
  return extra;
}

/** Build one-entry ZIP bytes, preserving VideoPsalm's AES-256 container. */
export function buildVpcEntry(
  entryName: string,
  data: Buffer,
  options: VpcWriteOptions = {}
): Buffer {
  const password = options.password ?? VIDEOPSALM_VPC_PASSWORD;
  const encrypt = options.encrypt ?? true;
  const compressed = deflateRawSync(data, { level: 9 });
  const name = Buffer.from(entryName, "utf8");
  const now = dosDateTime(new Date());
  let method = 8;
  // Ionic.Zip sets the UTF-8 name flag only when the name needs it.
  let flags = /[^\x00-\x7f]/.test(entryName) ? 0x800 : 0;
  let payload: Buffer = compressed;
  let crc = crc32(data);
  let extra: Buffer = Buffer.alloc(0);
  if (encrypt) {
    const salt = randomBytes(16);
    const derived = pbkdf2Sync(password, salt, 1000, 32 * 2 + 2, "sha1");
    const ciphertext = aesCtrCrypt(derived.subarray(0, 32), compressed);
    const auth = createHmac("sha1", derived.subarray(32, 64)).update(ciphertext).digest().subarray(0, 10);
    payload = Buffer.concat([salt, derived.subarray(64, 66), ciphertext, auth]);
    method = 99;
    flags |= 1;
    crc = 0;
    extra = aesExtra();
  }
  const local = Buffer.alloc(30);
  local.writeUInt32LE(LOCAL_SIG, 0);
  local.writeUInt16LE(51, 4);
  local.writeUInt16LE(flags, 6);
  local.writeUInt16LE(method, 8);
  local.writeUInt16LE(now.time, 10);
  local.writeUInt16LE(now.date, 12);
  local.writeUInt32LE(crc, 14);
  local.writeUInt32LE(payload.length, 18);
  local.writeUInt32LE(data.length, 22);
  local.writeUInt16LE(name.length, 26);
  local.writeUInt16LE(extra.length, 28);
  const localRecord = Buffer.concat([local, name, extra, payload]);

  const central = Buffer.alloc(46);
  central.writeUInt32LE(CENTRAL_SIG, 0);
  central.writeUInt16LE(0x033f, 4); // made by Unix / ZIP 3.0
  central.writeUInt16LE(51, 6);
  central.writeUInt16LE(flags, 8);
  central.writeUInt16LE(method, 10);
  central.writeUInt16LE(now.time, 12);
  central.writeUInt16LE(now.date, 14);
  central.writeUInt32LE(crc, 16);
  central.writeUInt32LE(payload.length, 20);
  central.writeUInt32LE(data.length, 24);
  central.writeUInt16LE(name.length, 28);
  central.writeUInt16LE(extra.length, 30);
  central.writeUInt16LE(0, 32);
  central.writeUInt16LE(0, 34);
  central.writeUInt16LE(0, 36);
  central.writeUInt32LE(0, 38);
  central.writeUInt32LE(0, 42);
  const centralRecord = Buffer.concat([central, name, extra]);

  const eocd = Buffer.alloc(22);
  eocd.writeUInt32LE(EOCD_SIG, 0);
  eocd.writeUInt16LE(0, 4);
  eocd.writeUInt16LE(0, 6);
  eocd.writeUInt16LE(1, 8);
  eocd.writeUInt16LE(1, 10);
  eocd.writeUInt32LE(centralRecord.length, 12);
  eocd.writeUInt32LE(localRecord.length, 16);
  eocd.writeUInt16LE(0, 20);
  return Buffer.concat([localRecord, centralRecord, eocd]);
}

/**
 * Build a plain (unencrypted) multi-entry ZIP.
 *
 * VideoPsalm's service plan (`.vpagd`) is exactly that: a ZIP of per-item
 * projections plus the media blobs. This writer is used to produce such a
 * file (tests, and exporting a plan back to the native shape).
 */
export function buildZipEntries(entries: { name: string; data: Buffer }[]): Buffer {
  const now = dosDateTime(new Date());
  const locals: Buffer[] = [];
  const centrals: Buffer[] = [];
  let offset = 0;
  for (const entry of entries) {
    const name = Buffer.from(entry.name, "utf8");
    const compressed = deflateRawSync(entry.data, { level: 9 });
    const crc = crc32(entry.data);
    const flags = /[^\x00-\x7f]/.test(entry.name) ? 0x800 : 0;
    const local = Buffer.alloc(30);
    local.writeUInt32LE(LOCAL_SIG, 0);
    local.writeUInt16LE(20, 4);
    local.writeUInt16LE(flags, 6);
    local.writeUInt16LE(8, 8);
    local.writeUInt16LE(now.time, 10);
    local.writeUInt16LE(now.date, 12);
    local.writeUInt32LE(crc, 14);
    local.writeUInt32LE(compressed.length, 18);
    local.writeUInt32LE(entry.data.length, 22);
    local.writeUInt16LE(name.length, 26);
    local.writeUInt16LE(0, 28);
    locals.push(Buffer.concat([local, name, compressed]));

    const central = Buffer.alloc(46);
    central.writeUInt32LE(CENTRAL_SIG, 0);
    central.writeUInt16LE(0x033f, 4);
    central.writeUInt16LE(20, 6);
    central.writeUInt16LE(flags, 8);
    central.writeUInt16LE(8, 10);
    central.writeUInt16LE(now.time, 12);
    central.writeUInt16LE(now.date, 14);
    central.writeUInt32LE(crc, 16);
    central.writeUInt32LE(compressed.length, 20);
    central.writeUInt32LE(entry.data.length, 24);
    central.writeUInt16LE(name.length, 28);
    central.writeUInt16LE(0, 30);
    central.writeUInt16LE(0, 32);
    central.writeUInt16LE(0, 34);
    central.writeUInt16LE(0, 36);
    central.writeUInt32LE(0, 38);
    central.writeUInt32LE(offset, 42);
    centrals.push(Buffer.concat([central, name]));
    offset += 30 + name.length + compressed.length;
  }
  const localBytes = Buffer.concat(locals);
  const centralBytes = Buffer.concat(centrals);
  const eocd = Buffer.alloc(22);
  eocd.writeUInt32LE(EOCD_SIG, 0);
  eocd.writeUInt16LE(entries.length, 8);
  eocd.writeUInt16LE(entries.length, 10);
  eocd.writeUInt32LE(centralBytes.length, 12);
  eocd.writeUInt32LE(localBytes.length, 16);
  return Buffer.concat([localBytes, centralBytes, eocd]);
}
