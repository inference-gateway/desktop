// Waveform peaks: the loudest sample in every 1/perSec of audio, computed
// once per file, then sliced per clip into one value per pixel column.
export const PEAKS_PER_SEC = 50;

export function computePeaks(samples: Float32Array, sampleRate: number, perSec = PEAKS_PER_SEC): Float32Array {
  const per = Math.max(1, Math.floor(sampleRate / perSec));
  const out = new Float32Array(Math.ceil(samples.length / per));
  for (let i = 0; i < out.length; i++) {
    let max = 0;
    const end = Math.min(samples.length, (i + 1) * per);
    for (let j = i * per; j < end; j++) {
      const v = Math.abs(samples[j]);
      if (v > max) max = v;
    }
    out[i] = max;
  }
  return out;
}

// One peak per column for the window [offset, offset + length) of the file.
export function peakColumns(
  peaks: Float32Array,
  offset: number,
  length: number,
  columns: number,
  perSec = PEAKS_PER_SEC,
): number[] {
  const out: number[] = [];
  for (let x = 0; x < columns; x++) {
    const from = Math.floor((offset + (x / columns) * length) * perSec);
    const to = Math.max(from + 1, Math.floor((offset + ((x + 1) / columns) * length) * perSec));
    let max = 0;
    for (let i = from; i < to && i < peaks.length; i++) if (peaks[i] > max) max = peaks[i];
    out.push(max);
  }
  return out;
}
