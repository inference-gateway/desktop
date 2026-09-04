import { expect, test } from "bun:test";
import { computePeaks, downsample, encodeWav, mergeChunks } from "./audio";

test("mergeChunks concatenates in order", () => {
  const out = mergeChunks([new Float32Array([1, 2]), new Float32Array([3])]);
  expect(Array.from(out)).toEqual([1, 2, 3]);
});

test("downsample halves length at a 2:1 ratio", () => {
  const out = downsample(new Float32Array(100).fill(0.5), 32000, 16000);
  expect(out.length).toBe(50);
  expect(out[0]).toBeCloseTo(0.5);
});

test("downsample is a no-op when target >= source", () => {
  const input = new Float32Array([0.1, 0.2]);
  expect(downsample(input, 16000, 16000)).toBe(input);
});

test("computePeaks buckets RMS energy normalized to the loudest bar", () => {
  const samples = new Float32Array(100);
  samples.fill(1, 0, 50);
  const peaks = computePeaks(samples, 4);
  expect(peaks.length).toBe(4);
  expect(peaks[0]).toBeCloseTo(1);
  expect(peaks[1]).toBeCloseTo(1);
  expect(peaks[3]).toBeCloseTo(0);
});

test("computePeaks handles empty input and silence", () => {
  expect(computePeaks(new Float32Array(0), 3)).toEqual([0, 0, 0]);
  expect(computePeaks(new Float32Array(10), 2)).toEqual([0, 0]);
});

test("encodeWav writes a valid 44-byte RIFF/WAVE header + PCM", () => {
  const wav = encodeWav(new Float32Array([0, 1, -1]), 16000);
  expect(wav.length).toBe(44 + 3 * 2);
  expect(String.fromCharCode(...wav.slice(0, 4))).toBe("RIFF");
  expect(String.fromCharCode(...wav.slice(8, 12))).toBe("WAVE");
  const view = new DataView(wav.buffer);
  expect(view.getUint32(24, true)).toBe(16000); // sample rate
  expect(view.getInt16(44 + 2, true)).toBe(0x7fff); // full-scale positive clamps
  expect(view.getInt16(44 + 4, true)).toBe(-0x8000); // full-scale negative clamps
});
