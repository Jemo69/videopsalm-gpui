// Media import filing: picked graphics land in Images/ or Videos/ under
// the document folder with a scrubbed, collision-free name.
import test from "node:test";
import assert from "node:assert/strict";
import * as fs from "node:fs";
import * as os from "node:os";
import * as path from "node:path";

import {
  copyMediaIntoFolder,
  freeMediaName,
  mediaImportDir,
  safeMediaName,
} from "../src/core/mediaImport";

function tmp(prefix: string): string {
  return fs.mkdtempSync(path.join(os.tmpdir(), prefix));
}

test("safe names strip separators and control characters", () => {
  assert.equal(safeMediaName("C:\\pics\\sunrise.jpg"), "sunrise.jpg");
  assert.equal(safeMediaName("../../etc/passwd"), "passwd");
  assert.equal(safeMediaName('a<b>:"c".jpg'), "a_b___c_.jpg");
});

test("free names never overwrite", () => {
  const dir = tmp("vp-medianame-");
  try {
    assert.equal(freeMediaName(dir, "pic.jpg"), "pic.jpg");
    fs.writeFileSync(path.join(dir, "pic.jpg"), "x");
    assert.equal(freeMediaName(dir, "pic.jpg"), "pic (2).jpg");
  } finally {
    fs.rmSync(dir, { recursive: true, force: true });
  }
});

test("copies images and videos into the right folders", () => {
  const src = tmp("vp-mediasrc-");
  const docs = tmp("vp-mediadocs-");
  try {
    const img = path.join(src, "sunrise.jpg");
    const vid = path.join(src, "loop.mp4");
    fs.writeFileSync(img, "img");
    fs.writeFileSync(vid, "vid");
    const res = copyMediaIntoFolder([img, vid, path.join(src, "notes.txt")], docs);
    assert.equal(res.copied.length, 2);
    assert.deepEqual(
      res.copied.map((c) => c.kind).sort(),
      ["image", "video"]
    );
    assert.ok(res.copied[0].path.startsWith(path.join(docs, "Images")));
    assert.ok(res.copied[1].path.startsWith(path.join(docs, "Videos")));
    assert.equal(res.errors.length, 1);
    assert.match(res.errors[0], /Not a media file/);
    // A second import of the same file does not overwrite the first.
    const again = copyMediaIntoFolder([img], docs);
    assert.match(again.copied[0].path, /\(2\)\.jpg$/);
  } finally {
    fs.rmSync(src, { recursive: true, force: true });
    fs.rmSync(docs, { recursive: true, force: true });
  }
});

test("mediaImportDir creates Images and Videos on demand", () => {
  const docs = tmp("vp-mediadir-");
  try {
    assert.equal(mediaImportDir(docs, "image"), path.join(docs, "Images"));
    assert.equal(mediaImportDir(docs, "video"), path.join(docs, "Videos"));
    assert.ok(fs.statSync(path.join(docs, "Images")).isDirectory());
  } finally {
    fs.rmSync(docs, { recursive: true, force: true });
  }
});
