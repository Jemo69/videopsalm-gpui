/**
 * B-32 galleries + transport pure logic (Electron port).
 *
 * Mirrors `gpui-app/vp-core/src/media.rs`'s test block: same rules, both
 * ports. Only the pure functions are imported here — `mountGalleries` needs
 * a DOM and is thin glue over the hooks.
 */
import test from "node:test";
import assert from "node:assert/strict";
import {
  BLACK_FLASH_DEFAULT_MS,
  BLACK_SENTINEL_ID,
  GalleryEntry,
  assignedBgKind,
  brightnessFilter,
  dirOf,
  fileCaption,
  filterGallery,
  galleryChain,
  luminosityLevel,
  luminosityToFilter,
  parseRange,
  resolveGalleryLuminosity,
  setGalleryLuminosity,
  stepTransport,
} from "../src/renderer/media-gallery.js";
import { emptyCascade } from "../src/core/cascade.js";

const IMG: GalleryEntry = { id: "m1", path: "/pics/Red Sunset.jpg", kind: "image", blackFlashMs: BLACK_FLASH_DEFAULT_MS };
const VID: GalleryEntry = { id: "m2", path: "/v/Blue Ocean Loop.mp4", kind: "video", blackFlashMs: BLACK_FLASH_DEFAULT_MS };

test("caption is the file name, never blank", () => {
  assert.equal(fileCaption(IMG.path), "Red Sunset.jpg");
  assert.equal(fileCaption(""), "(unnamed)");
  assert.equal(fileCaption("/v/4a1a7a75-5346.mp4"), "4a1a7a75-5346.mp4");
});

test("dirOf names the containing folder for the Explore row", () => {
  assert.equal(dirOf("/pics/Red Sunset.jpg"), "/pics");
  assert.equal(dirOf("C:\\Users\\Public\\VideoPsalm\\x.mp4"), "C:/Users/Public/VideoPsalm");
  assert.equal(dirOf("bare.mp4"), "");
});

test("black flash defaults to 500 ms per video (RESEARCH §12.3)", () => {
  assert.equal(BLACK_FLASH_DEFAULT_MS, 500);
  assert.equal(VID.blackFlashMs, 500);
  assert.equal(BLACK_SENTINEL_ID, "black");
});

test("filter matches filename words order-free, empty query passes all", () => {
  assert.deepEqual(filterGallery([IMG, VID], ""), [IMG, VID]);
  assert.deepEqual(filterGallery([IMG, VID], "sunset red"), [IMG]);
  assert.deepEqual(filterGallery([IMG, VID], "LOOP blue"), [VID]);
  assert.deepEqual(filterGallery([IMG, VID], "desert"), []);
});

test("assignment kind follows the gallery (§6.2)", () => {
  assert.equal(assignedBgKind("image"), "image");
  assert.equal(assignedBgKind("video"), "videoLoop");
});

test("media resolve through their own chains (B-33)", () => {
  assert.equal(galleryChain("image"), "images");
  assert.equal(galleryChain("video"), "videos");
  assert.equal(luminosityLevel("image"), "images");
  assert.equal(luminosityLevel("video"), "videos");
});

test("luminosity writes at the media collection level and stays out of the song chain", () => {
  let c = emptyCascade();
  assert.equal(resolveGalleryLuminosity(c, "video").value, 0);
  assert.equal(resolveGalleryLuminosity(c, "video").source, undefined);
  c = setGalleryLuminosity(c, "video", 3);
  const v = resolveGalleryLuminosity(c, "video");
  assert.equal(v.value, 3);
  assert.equal(v.source, "videos");
  // Images are a different chain: untouched.
  assert.equal(resolveGalleryLuminosity(c, "image").source, undefined);
  c = setGalleryLuminosity(c, "image", -2);
  assert.equal(resolveGalleryLuminosity(c, "image").value, -2);
});

test("brightness filter is neutral at zero and clamps", () => {
  assert.equal(luminosityToFilter(0), "");
  assert.ok(luminosityToFilter(2).startsWith("brightness(1."));
  assert.ok(luminosityToFilter(-2).startsWith("brightness(0."));
  assert.equal(luminosityToFilter(1000), "brightness(2.00)");
  assert.equal(brightnessFilter(emptyCascade(), "color"), "");
  const c = setGalleryLuminosity(emptyCascade(), "video", 2);
  assert.ok(brightnessFilter(c, "videoLoop").startsWith("brightness("));
  assert.equal(brightnessFilter(c, "image"), "");
});

test("transport machine: play/pause/stop/toggle", () => {
  assert.equal(stepTransport("stopped", "toggle"), "playing");
  assert.equal(stepTransport("playing", "toggle"), "paused");
  assert.equal(stepTransport("paused", "toggle"), "playing");
  assert.equal(stepTransport("playing", "pause"), "paused");
  assert.equal(stepTransport("stopped", "pause"), "stopped");
  assert.equal(stepTransport("playing", "stop"), "stopped");
  assert.equal(stepTransport("paused", "play"), "playing");
});

test("range parses in-out milliseconds, rejects garbage", () => {
  assert.deepEqual(parseRange("1000-5000"), { startMs: 1000, endMs: 5000 });
  assert.deepEqual(parseRange("500-"), { startMs: 500, endMs: null });
  assert.deepEqual(parseRange("-9000"), { startMs: null, endMs: 9000 });
  assert.deepEqual(parseRange("abc-def"), { startMs: null, endMs: null });
  assert.deepEqual(parseRange(""), { startMs: null, endMs: null });
});
