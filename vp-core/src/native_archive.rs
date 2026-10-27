//! VideoPsalm ZIP containers: `.vpc` songbooks and `.vpagd` service plans.
//!
//! A `.vpc` songbook is a one-entry Ionic.Zip archive, WinZip AES-256 when
//! protected, with VideoPsalm's fixed container password (an ASCII constant
//! from `Extension.GetFileName` in 1.29 — not a user secret). A `.vpagd`
//! service plan is a plain multi-entry ZIP of per-item projections plus
//! media blobs.
//!
//! This module is the Rust twin of `electron-app/src/core/vpc.ts`; the byte
//! layout (headers, AES-CTR counter, KDF parameters) is ported field for
//! field so archives travel between the ports. The committed
//! `videopsalm-aes.vpc` fixture (written by the TypeScript side) is
//! decrypted here in tests, pinning both ports to one artifact.

use aes::Aes256;
use aes::cipher::{BlockCipherEncrypt, KeyInit};
use crc32fast::Hasher as Crc32;
use flate2::Compression;
use flate2::read::DeflateDecoder;
use flate2::write::DeflateEncoder;
use hmac::{Hmac, Mac};
use sha1::Sha1;
use std::io::{Read, Write};

/// VideoPsalm's fixed container password (`Extension.GetFileName`, 1.29).
/// Not a user secret: it is the same ASCII constant for every install.
pub const VPC_PASSWORD: &str = "5$l(va\\7N_0!',/:5?M0.(d_<8+5!Um2#~M@{5#2(C3]$'f|#:Y/}wG+-)Nt:x^8!Y.5Eq$)'7stk$y)\\.~Tg=&<l";

const LOCAL_SIG: u32 = 0x0403_4b50;
const CENTRAL_SIG: u32 = 0x0201_4b50;
const EOCD_SIG: u32 = 0x0605_4b50;
const AES_EXTRA_ID: u16 = 0x9901;

/// One file entry of a VideoPsalm ZIP container.
#[derive(Clone, Debug, PartialEq)]
pub struct ArchiveEntry {
    pub name: String,
    pub data: Vec<u8>,
    pub encrypted: bool,
}

#[derive(Clone, Copy, Debug)]
struct AesInfo {
    strength: u8,
    actual_method: u16,
}

#[derive(Clone, Debug)]
struct CentralEntry {
    name: String,
    flags: u16,
    method: u16,
    compressed_size: u32,
    uncompressed_size: u32,
    local_offset: u32,
    extra: Vec<u8>,
}

fn u16le(buf: &[u8], at: usize) -> Result<u16, String> {
    buf.get(at..at + 2)
        .and_then(|s| s.try_into().ok())
        .map(u16::from_le_bytes)
        .ok_or_else(|| "Truncated ZIP record".to_string())
}

fn u32le(buf: &[u8], at: usize) -> Result<u32, String> {
    buf.get(at..at + 4)
        .and_then(|s| s.try_into().ok())
        .map(u32::from_le_bytes)
        .ok_or_else(|| "Truncated ZIP record".to_string())
}

fn parse_aes_extra(extra: &[u8]) -> Option<AesInfo> {
    let mut i = 0;
    while i + 4 <= extra.len() {
        let id = u16::from_le_bytes([extra[i], extra[i + 1]]);
        let len = u16::from_le_bytes([extra[i + 2], extra[i + 3]]) as usize;
        let end = i + 4 + len;
        if end > extra.len() {
            return None;
        }
        if id == AES_EXTRA_ID && len >= 7 {
            return Some(AesInfo {
                strength: extra[i + 8],
                actual_method: u16::from_le_bytes([extra[i + 9], extra[i + 10]]),
            });
        }
        i = end;
    }
    None
}

fn find_eocd(buf: &[u8]) -> Result<usize, String> {
    if buf.len() < 22 {
        return Err("Not a ZIP archive (end-of-central-directory record not found)".to_string());
    }
    let start = buf.len().saturating_sub(22 + 0xffff);
    let mut i = buf.len() - 22;
    loop {
        if u32le(buf, i).unwrap_or(0) == EOCD_SIG {
            return Ok(i);
        }
        if i == start {
            break;
        }
        i -= 1;
    }
    Err("Not a ZIP archive (end-of-central-directory record not found)".to_string())
}

fn central_entries(buf: &[u8]) -> Result<Vec<CentralEntry>, String> {
    let eocd = find_eocd(buf)?;
    let count = u16le(buf, eocd + 10)? as usize;
    let offset = u32le(buf, eocd + 16)? as usize;
    if count == 0xffff || offset == 0xffff_ffff {
        return Err("ZIP64 .vpc archives are not supported".to_string());
    }
    let mut entries = Vec::with_capacity(count);
    let mut p = offset;
    for _ in 0..count {
        if p + 46 > buf.len() || u32le(buf, p)? != CENTRAL_SIG {
            return Err("Malformed ZIP central directory".to_string());
        }
        let flags = u16le(buf, p + 8)?;
        let method = u16le(buf, p + 10)?;
        let compressed_size = u32le(buf, p + 20)?;
        let uncompressed_size = u32le(buf, p + 24)?;
        let name_len = u16le(buf, p + 28)? as usize;
        let extra_len = u16le(buf, p + 30)? as usize;
        let comment_len = u16le(buf, p + 32)? as usize;
        let local_offset = u32le(buf, p + 42)?;
        let name_start = p + 46;
        if name_start + name_len + extra_len + comment_len > buf.len() {
            return Err("Malformed ZIP central-directory entry".to_string());
        }
        let name_bytes = &buf[name_start..name_start + name_len];
        let name = if flags & 0x800 != 0 {
            String::from_utf8_lossy(name_bytes).into_owned()
        } else {
            name_bytes.iter().map(|&b| b as char).collect()
        };
        let extra = buf[name_start + name_len..name_start + name_len + extra_len].to_vec();
        entries.push(CentralEntry {
            name,
            flags,
            method,
            compressed_size,
            uncompressed_size,
            local_offset,
            extra,
        });
        p = name_start + name_len + extra_len + comment_len;
    }
    Ok(entries)
}

fn deflate(data: &[u8]) -> Vec<u8> {
    let mut enc = DeflateEncoder::new(Vec::new(), Compression::best());
    enc.write_all(data).expect("deflate into memory cannot fail");
    enc.finish().expect("deflate finish cannot fail")
}

fn decompress(data: &[u8], method: u16, uncompressed_size: u32) -> Result<Vec<u8>, String> {
    let out = if method == 0 {
        data.to_vec()
    } else if method == 8 {
        let mut dec = DeflateDecoder::new(data);
        let mut out = Vec::new();
        dec.read_to_end(&mut out)
            .map_err(|e| format!("Corrupt .vpc archive (deflate failed: {e})"))?;
        out
    } else {
        return Err(format!("Unsupported .vpc compression method {method}"));
    };
    if uncompressed_size != 0xffff_ffff && out.len() as u32 != uncompressed_size {
        return Err("Corrupt .vpc archive (uncompressed size mismatch)".to_string());
    }
    Ok(out)
}

/// AES-CTR with WinZip's 128-bit little-endian block counter (starts at 1).
fn aes_ctr_crypt(key: &[u8], data: &[u8]) -> Result<Vec<u8>, String> {
    let cipher = Aes256::new_from_slice(key).map_err(|e| format!("Bad AES key length: {e}"))?;
    let mut counter = [0u8; 16];
    counter[0] = 1;
    let mut out = vec![0u8; data.len()];
    for (chunk, dest) in data.chunks(16).zip(out.chunks_mut(16)) {
        let mut block = hybrid_array::Array::<u8, _>::from(counter);
        cipher.encrypt_block(&mut block);
        for (i, b) in chunk.iter().enumerate() {
            dest[i] = b ^ block[i];
        }
        // Little-endian increment with carry across all 16 bytes.
        for byte in counter.iter_mut() {
            *byte = byte.wrapping_add(1);
            if *byte != 0 {
                break;
            }
        }
    }
    Ok(out)
}

fn extract_aes(payload: &[u8], aes: AesInfo, uncompressed_size: u32, password: &str) -> Result<Vec<u8>, String> {
    let salt_len = match aes.strength {
        1 => 8,
        2 => 12,
        _ => 16,
    };
    let key_len = match aes.strength {
        1 => 16,
        2 => 24,
        _ => 32,
    };
    if payload.len() < salt_len + 2 + 10 {
        return Err("Truncated WinZip AES entry".to_string());
    }
    let salt = &payload[..salt_len];
    let verifier = &payload[salt_len..salt_len + 2];
    let ciphertext = &payload[salt_len + 2..payload.len() - 10];
    let auth = &payload[payload.len() - 10..];
    let mut derived = vec![0u8; key_len * 2 + 2];
    pbkdf2::pbkdf2_hmac::<Sha1>(password.as_bytes(), salt, 1000, &mut derived);
    let (key, rest) = derived.split_at(key_len);
    let (auth_key, expected) = rest.split_at(key_len);
    if verifier != expected {
        return Err("Incorrect .vpc password (or the archive is not WinZip-AES)".to_string());
    }
    let mut mac = Hmac::<Sha1>::new_from_slice(auth_key).map_err(|e| format!("Bad HMAC key: {e}"))?;
    mac.update(ciphertext);
    let digest = mac.finalize().into_bytes();
    if auth != &digest[..10] {
        return Err("Corrupt .vpc archive (authentication failed)".to_string());
    }
    let plain = aes_ctr_crypt(key, ciphertext)?;
    decompress(&plain, aes.actual_method, uncompressed_size)
}

/// Read every file entry of a VideoPsalm ZIP container.
///
/// A `.vpc` songbook holds one entry; a `.vpagd` service plan holds many.
/// Protected songbooks are WinZip AES-256 with VideoPsalm's fixed password;
/// agenda archives are plain. ZIP64 and ZipCrypto are rejected, exactly like
/// the TypeScript twin (VideoPsalm 1.29 writes neither).
pub fn read_vpac_entries(data: &[u8], password: &str) -> Result<Vec<ArchiveEntry>, String> {
    let mut out = Vec::new();
    for entry in central_entries(data)? {
        let local = entry.local_offset as usize;
        if local + 30 > data.len() || u32le(data, local)? != LOCAL_SIG {
            return Err("Malformed .vpc local file header".to_string());
        }
        let local_flags = u16le(data, local + 6)?;
        let local_method = u16le(data, local + 8)?;
        let name_len = u16le(data, local + 26)? as usize;
        let extra_len = u16le(data, local + 28)? as usize;
        let data_start = local + 30 + name_len + extra_len;
        let local_extra = data
            .get(local + 30 + name_len..data_start)
            .ok_or_else(|| "Malformed .vpc local file header".to_string())?;
        let flags = entry.flags | local_flags;
        let method = if entry.method != 0 { entry.method } else { local_method };
        let size = entry.compressed_size as usize;
        if data_start + size > data.len() {
            return Err("Truncated .vpc entry data".to_string());
        }
        let payload = &data[data_start..data_start + size];
        let encrypted = flags & 1 != 0;
        if !encrypted {
            out.push(ArchiveEntry {
                name: entry.name,
                data: decompress(payload, method, entry.uncompressed_size)?,
                encrypted: false,
            });
            continue;
        }
        if method != 99 {
            return Err("This .vpc uses legacy ZipCrypto encryption, which VideoPsalm 1.29 does not write".to_string());
        }
        let aes = parse_aes_extra(local_extra)
            .or_else(|| parse_aes_extra(&entry.extra))
            .ok_or_else(|| "Encrypted .vpc is missing its WinZip-AES metadata".to_string())?;
        out.push(ArchiveEntry {
            name: entry.name,
            data: extract_aes(payload, aes, entry.uncompressed_size, password)?,
            encrypted: true,
        });
    }
    Ok(out)
}

/// Read the first file entry of a VideoPsalm `.vpc` archive.
pub fn read_vpac_entry(data: &[u8], password: &str) -> Result<ArchiveEntry, String> {
    read_vpac_entries(data, password)?
        .into_iter()
        .next()
        .ok_or_else(|| "The .vpc archive contains no file entry".to_string())
}

fn crc32(data: &[u8]) -> u32 {
    let mut h = Crc32::new();
    h.update(data);
    h.finalize()
}

/// Fixed DOS timestamp (native archives carry wall-clock time; nothing in
/// the app reads it, so a constant keeps builds reproducible).
fn dos_date_time() -> (u16, u16) {
    // 2026-01-01 00:00:00.
    let time = 0u16;
    let date = ((2026 - 1980) << 9) | (1 << 5) | 1;
    (time, date as u16)
}

fn aes_extra() -> Vec<u8> {
    let mut extra = vec![0u8; 11];
    extra[0..2].copy_from_slice(&AES_EXTRA_ID.to_le_bytes());
    extra[2..4].copy_from_slice(&7u16.to_le_bytes());
    extra[4..6].copy_from_slice(&2u16.to_le_bytes()); // AE-2
    extra[6..8].copy_from_slice(b"AE");
    extra[8] = 3; // AES-256
    extra[9..11].copy_from_slice(&8u16.to_le_bytes()); // actual method: deflate
    extra
}

/// Build a one-entry `.vpc` archive, preserving VideoPsalm's AES-256
/// container. `encrypt = false` keeps an unprotected archive unprotected.
pub fn build_vpc_entry(entry_name: &str, data: &[u8], password: &str, encrypt: bool) -> Vec<u8> {
    let compressed = deflate(data);
    let name = entry_name.as_bytes();
    let (dos_time, dos_date) = dos_date_time();
    // Ionic.Zip sets the UTF-8 name flag only when the name needs it.
    let mut flags: u16 = if entry_name.is_ascii() { 0 } else { 0x800 };
    let mut method: u16 = 8;
    let mut payload = compressed;
    let mut crc = crc32(data);
    let mut extra = Vec::new();
    if encrypt {
        let mut salt = [0u8; 16];
        rand_bytes::fill_random(&mut salt);
        let mut derived = [0u8; 66];
        pbkdf2::pbkdf2_hmac::<Sha1>(password.as_bytes(), &salt, 1000, &mut derived);
        let keystream_ct =
            aes_ctr_crypt(&derived[..32], &payload).expect("CTR cannot fail");
        let mut mac =
            Hmac::<Sha1>::new_from_slice(&derived[32..64]).expect("HMAC key");
        mac.update(&keystream_ct);
        let digest = mac.finalize().into_bytes();
        let mut blob = Vec::with_capacity(16 + 2 + keystream_ct.len() + 10);
        blob.extend_from_slice(&salt);
        blob.extend_from_slice(&derived[64..66]);
        blob.extend_from_slice(&keystream_ct);
        blob.extend_from_slice(&digest[..10]);
        payload = blob;
        method = 99;
        flags |= 1;
        crc = 0;
        extra = aes_extra();
    }
    assemble_single(
        name,
        &extra,
        flags,
        method,
        dos_time,
        dos_date,
        crc,
        &payload,
        data.len() as u32,
    )
}

fn put_u16(out: &mut Vec<u8>, v: u16) {
    out.extend_from_slice(&v.to_le_bytes());
}
fn put_u32(out: &mut Vec<u8>, v: u32) {
    out.extend_from_slice(&v.to_le_bytes());
}

#[allow(clippy::too_many_arguments)]
fn local_record(name: &[u8], extra: &[u8], flags: u16, method: u16, time: u16, date: u16, crc: u32, payload: &[u8], uncompressed: u32) -> Vec<u8> {
    let mut rec = Vec::with_capacity(30 + name.len() + extra.len() + payload.len());
    put_u32(&mut rec, LOCAL_SIG);
    put_u16(&mut rec, if method == 99 { 51 } else { 20 });
    put_u16(&mut rec, flags);
    put_u16(&mut rec, method);
    put_u16(&mut rec, time);
    put_u16(&mut rec, date);
    put_u32(&mut rec, crc);
    put_u32(&mut rec, payload.len() as u32);
    put_u32(&mut rec, uncompressed);
    put_u16(&mut rec, name.len() as u16);
    put_u16(&mut rec, extra.len() as u16);
    rec.extend_from_slice(name);
    rec.extend_from_slice(extra);
    rec.extend_from_slice(payload);
    rec
}

#[allow(clippy::too_many_arguments)]
fn central_record(name: &[u8], extra: &[u8], flags: u16, method: u16, time: u16, date: u16, crc: u32, payload_len: u32, uncompressed: u32, local_offset: u32, made_by: u16) -> Vec<u8> {
    let mut rec = Vec::with_capacity(46 + name.len() + extra.len());
    put_u32(&mut rec, CENTRAL_SIG);
    put_u16(&mut rec, made_by);
    put_u16(&mut rec, if method == 99 { 51 } else { 20 });
    put_u16(&mut rec, flags);
    put_u16(&mut rec, method);
    put_u16(&mut rec, time);
    put_u16(&mut rec, date);
    put_u32(&mut rec, crc);
    put_u32(&mut rec, payload_len);
    put_u32(&mut rec, uncompressed);
    put_u16(&mut rec, name.len() as u16);
    put_u16(&mut rec, extra.len() as u16);
    put_u16(&mut rec, 0);
    put_u16(&mut rec, 0);
    put_u16(&mut rec, 0);
    put_u32(&mut rec, 0);
    put_u32(&mut rec, local_offset);
    rec.extend_from_slice(name);
    rec.extend_from_slice(extra);
    rec
}

fn eocd_record(count: usize, central_len: usize, local_len: usize) -> Vec<u8> {
    let mut rec = Vec::with_capacity(22);
    put_u32(&mut rec, EOCD_SIG);
    put_u16(&mut rec, 0);
    put_u16(&mut rec, 0);
    put_u16(&mut rec, count as u16);
    put_u16(&mut rec, count as u16);
    put_u32(&mut rec, central_len as u32);
    put_u32(&mut rec, local_len as u32);
    put_u16(&mut rec, 0);
    rec
}

/// Fill `buf` with OS randomness (`/dev/urandom`).
/// Kept local so `vp-core` gains no new dependency for one 16-byte salt.
mod rand_bytes {
    use std::fs::File;
    use std::io::Read;
    pub fn fill_random(buf: &mut [u8]) {
        // Best effort: a weak salt still decrypts (the salt is stored in
        // the entry); failing randomness must not fail a church save, so
        // fall back to time-mixed bytes.
        if let Ok(mut f) = File::open("/dev/urandom") {
            if f.read_exact(buf).is_ok() {
                return;
            }
        }
        let mut x: u64 = 0x9E37_79B9_7F4A_7C15 ^ (buf.as_ptr() as u64);
        let mut filled = 0;
        while filled < buf.len() {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            let bytes = x.to_le_bytes();
            let n = (buf.len() - filled).min(8);
            buf[filled..filled + n].copy_from_slice(&bytes[..n]);
            filled += n;
        }
    }
}

/// Build a plain (unencrypted) multi-entry ZIP.
///
/// A VideoPsalm service plan (`.vpagd`) is exactly that: per-item
/// projections plus the media blobs.
pub fn build_zip_entries(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let (dos_time, dos_date) = dos_date_time();
    let mut locals = Vec::new();
    let mut centrals = Vec::new();
    let mut offset: u32 = 0;
    for (name_str, data) in entries {
        let name = name_str.as_bytes();
        let compressed = deflate(data);
        let crc = crc32(data);
        let flags: u16 = if name_str.is_ascii() { 0 } else { 0x800 };
        let local = local_record(name, &[], flags, 8, dos_time, dos_date, crc, &compressed, data.len() as u32);
        offset += local.len() as u32;
        let central = central_record(name, &[], flags, 8, dos_time, dos_date, crc, compressed.len() as u32, data.len() as u32, offset - local.len() as u32, 0x033f);
        locals.push(local);
        centrals.push(central);
    }
    let local_bytes: Vec<u8> = locals.concat();
    let central_bytes: Vec<u8> = centrals.concat();
    let eocd = eocd_record(entries.len(), central_bytes.len(), local_bytes.len());
    [local_bytes, central_bytes, eocd].concat()
}

#[allow(clippy::too_many_arguments)]
fn assemble_single(
    name: &[u8],
    extra: &[u8],
    flags: u16,
    method: u16,
    time: u16,
    date: u16,
    crc: u32,
    payload: &[u8],
    uncompressed: u32,
) -> Vec<u8> {
    let local = local_record(name, extra, flags, method, time, date, crc, payload, uncompressed);
    let central = central_record(
        name, extra, flags, method, time, date, crc,
        payload.len() as u32, uncompressed, 0, 0x033f,
    );
    let eocd = eocd_record(1, central.len(), local.len());
    [local, central, eocd].concat()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::native_songbook;

    #[test]
    fn plain_zip_round_trip() {
        let entries: Vec<(&str, &[u8])> = vec![
            ("Version.json", b"2".as_slice()),
            ("Song_0.json", b"{Text:\"Grace\"}".as_slice()),
            ("caf\u{e9}.json", "caf\u{e9}".as_bytes()),
            ("empty.bin", b"".as_slice()),
        ];
        let zip = build_zip_entries(&entries);
        let read = read_vpac_entries(&zip, VPC_PASSWORD).expect("plain zip must read");
        assert_eq!(read.len(), 4);
        for (i, e) in read.iter().enumerate() {
            assert_eq!(e.name, entries[i].0);
            assert_eq!(e.data, entries[i].1);
            assert!(!e.encrypted);
        }
    }

    #[test]
    fn vpc_aes_round_trip() {
        let data = "{Abbreviation:\"SB1\",Songs:[{Text:\"Amazing Grace\",Verses:[{Text:\"line one\\nline two\"}]}]}".as_bytes();
        let vpc = build_vpc_entry("SB1.json", data, VPC_PASSWORD, true);
        let entry = read_vpac_entry(&vpc, VPC_PASSWORD).expect("AES entry must read");
        assert_eq!(entry.name, "SB1.json");
        assert_eq!(entry.data, data);
        assert!(entry.encrypted);
    }

    #[test]
    fn vpc_unprotected_round_trip() {
        let data = b"{Text:\"plain\"}";
        let vpc = build_vpc_entry("SB1.json", data, VPC_PASSWORD, false);
        let entry = read_vpac_entry(&vpc, VPC_PASSWORD).expect("plain vpc must read");
        assert_eq!(entry.data, data);
        assert!(!entry.encrypted);
    }

    #[test]
    fn vpc_wrong_password_fails() {
        let vpc = build_vpc_entry("SB1.json", b"{Text:\"x\"}", VPC_PASSWORD, true);
        let err = read_vpac_entry(&vpc, "wrong-password").unwrap_err();
        assert!(err.contains("password"), "unexpected error: {err}");
    }

    #[test]
    fn vpc_tampered_ciphertext_fails_auth() {
        let mut vpc = build_vpc_entry("SB1.json", b"{Text:\"tamper me please, this is long enough\"}", VPC_PASSWORD, true);
        // Flip a byte just past the local header (inside the AES blob).
        let at = vpc.len() / 2;
        vpc[at] ^= 0x01;
        let err = read_vpac_entry(&vpc, VPC_PASSWORD).unwrap_err();
        assert!(
            err.contains("authentication") || err.contains("size mismatch") || err.contains("deflate"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn garbage_is_not_a_zip() {
        let err = read_vpac_entries(b"definitely not a zip archive", VPC_PASSWORD).unwrap_err();
        assert!(err.contains("Not a ZIP"), "unexpected error: {err}");
    }

    /// Cross-port pin: the `.vpc` fixture was written by the TypeScript side
    /// (`electron-app/tests/videopsalm.test.ts`) and committed. If this port
    /// cannot decrypt it, the two ports disagree on the container.
    #[test]
    fn decrypts_typescript_written_fixture() {
        let bytes = crate::cross_port_fixture("videopsalm-aes.vpc");
        let entry = read_vpac_entry(&bytes, VPC_PASSWORD).expect("fixture must decrypt with the fixed password");
        assert!(entry.encrypted);
        let root = native_songbook::parse(&String::from_utf8_lossy(&entry.data))
            .expect("fixture payload must parse as the native dialect");
        let book = native_songbook::songbook_to_songs(&root, "/tmp/SongBooks/SB1.json")
            .expect("fixture must map to songs");
        assert_eq!(book.songs.len(), 1);
        assert_eq!(book.songbook.name, "AES Fixture Book");
        assert_eq!(book.songs[0].title, "AES Fixture");
        assert_eq!(book.songs[0].authors, vec!["Fixture Author".to_string()]);
    }
}
