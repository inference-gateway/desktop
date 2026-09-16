import { expect, test } from "bun:test";
import { computePeaks, peakColumns } from "./peaks";

test("computePeaks keeps the loudest sample per bucket and peakColumns slices a window", () => {
  const samples = new Float32Array(400);
  samples[10] = -0.5;
  samples[150] = 0.75;
  samples[399] = 0.25;
  const peaks = computePeaks(samples, 100, 1);
  expect(Array.from(peaks)).toEqual([0.5, 0.75, 0, 0.25]);
  expect(peakColumns(peaks, 0, 4, 2, 1)).toEqual([0.75, 0.25]);
  expect(peakColumns(peaks, 1, 1, 1, 1)).toEqual([0.75]);
  expect(peakColumns(peaks, 3, 5, 2, 1)).toEqual([0.25, 0]);
});
