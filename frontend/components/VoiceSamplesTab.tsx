import { useCallback, useEffect, useRef, useState } from "react";
import { AlertTriangle, CheckCircle2, Loader2, Mic, Square, Trash2, Volume2 } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { cn } from "@/lib/utils";
import { api, type VoiceSample } from "@/lib/tauri";
import { AudioPlayer } from "@/components/AudioPlayer";
import { safeAudioSrc } from "@/lib/tools";
import {
  CLIPPING_PEAK,
  QUIET_PEAK,
  type SampleIssue,
  encodeWav,
  mergeChunks,
  peakLevel,
  sampleIssues,
  trimSilence,
} from "@/lib/audio";
import { isLocalSpeechModel, speechModelLabel } from "@/lib/speech-models";

// Managed library of WAV reference recordings for voice cloning, backed by the
// list/add/delete voice-sample commands (backend/src/tts_samples.rs). Upload is
// a native file picker + copy, or an in-app mic recording saved via
// save_voice_sample; preview streams through the asset protocol.
const MAX_SAMPLE_REC_MS = 30000;

// Suggested readings for the record flow, sized to ~10-13s at a natural pace.
// One sample per style; the style name doubles as the sample file name so four
// takes don't overwrite each other. Uploaded WAVs count toward the same target.
const SAMPLE_READINGS = [
  {
    style: "Conversational",
    tip: "Warm and relaxed, as if explaining to a friend.",
    text: "Let me show you how this works. We start with a simple idea, try it out, and make a few adjustments along the way.",
  },
  {
    style: "Enthusiastic",
    tip: "Smile slightly, with brighter energy and emphasis.",
    text: "Here's the exciting part! With just one small change, everything starts working together. Suddenly, something that seemed complicated becomes surprisingly simple. Let's see it in action!",
  },
  {
    style: "Thoughtful and curious",
    tip: "Slow slightly at the question, then pause before continuing.",
    text: "But why does this happen? At first, the answer seems obvious. Look a little closer, though, and you'll notice something we haven't considered yet.",
  },
  {
    style: "Confident and clear",
    tip: "Steady and assured, emphasizing the main takeaway.",
    text: "The key is to start small. Focus on one problem, check your results, and build from there. Each step gives you a stronger foundation.",
  },
];

const RECOMMENDED_SAMPLES = SAMPLE_READINGS.length;

const TEST_READING =
  "Here's the idea. You describe the change you want, and the agent plans it, writes the code, and shows you every step before anything runs. It sounds simple, and it is. But why does that matter? Because you stay in control the whole time, from the first line to the last.";

const ISSUE_ADVICE: Record<SampleIssue, string> = {
  "too short": "Under 10 seconds - record a longer take.",
  "too quiet": "Very quiet - move closer to the microphone.",
  clipped: "Distorted peaks - move back or lower the input level.",
};

// A cloning reference wants the raw microphone: the browser's own processing smears the voice.
const RAW_MICROPHONE = { echoCancellation: false, noiseSuppression: false, autoGainControl: false };

type SampleQuality = { seconds: number; issues: SampleIssue[] };
type SpeechSetup = { enabled: boolean; model: string };

function newAudioContext(): AudioContext {
  const Ctx: typeof AudioContext =
    window.AudioContext || (window as unknown as { webkitAudioContext: typeof AudioContext }).webkitAudioContext;
  return new Ctx();
}

function useSampleQuality(src: string | null): SampleQuality | null {
  const [quality, setQuality] = useState<SampleQuality | null>(null);
  useEffect(() => {
    if (!src) return;
    let alive = true;
    (async () => {
      try {
        const ctx = newAudioContext();
        const decoded = await ctx.decodeAudioData(await (await fetch(src)).arrayBuffer());
        const issues = sampleIssues(decoded.getChannelData(0), decoded.sampleRate);
        if (alive) setQuality({ seconds: decoded.duration, issues });
        await ctx.close();
      } catch {
        if (alive) setQuality(null);
      }
    })();
    return () => {
      alive = false;
    };
  }, [src]);
  return quality;
}

function levelAdvice(level: number): { text: string; bar: string } {
  if (level >= CLIPPING_PEAK) return { text: "Too loud - move back", bar: "bg-destructive" };
  if (level < QUIET_PEAK) return { text: "Too quiet - move closer", bar: "bg-amber-500" };
  return { text: "Good level", bar: "bg-emerald-500" };
}

function LevelMeter({ level }: { level: number }) {
  const advice = levelAdvice(level);
  return (
    <div className="flex items-center gap-2" role="status" aria-label="Microphone level">
      <div className="h-2 w-28 overflow-hidden rounded bg-secondary">
        <div className={cn("h-full", advice.bar)} style={{ width: `${Math.min(1, level) * 100}%` }} />
      </div>
      <span className="text-[0.75rem] text-muted-foreground">{advice.text}</span>
    </div>
  );
}

function testHint(speech: SpeechSetup | null): string {
  if (!speech) return "";
  if (!speech.enabled) return "Text to speech is off. Turn it on in General to test a sample.";
  const reading = `Test reads the text below in a sample's voice with ${speechModelLabel(speech.model)}, exactly as an agent would.`;
  if (isLocalSpeechModel(speech.model)) return reading;
  return `${reading} A hosted model uploads the sample to its provider, once, to clone the voice there.`;
}

function SampleCard({
  sample,
  canTest,
  testing,
  test,
  testError,
  onTest,
  onDelete,
}: {
  sample: VoiceSample;
  canTest: boolean;
  testing: boolean;
  test?: { path: string; at: number };
  testError?: string;
  onTest: () => void;
  onDelete: () => void;
}) {
  const src = safeAudioSrc(sample.path);
  const quality = useSampleQuality(src);
  const testSrc = safeAudioSrc(test?.path);
  return (
    <div className="rounded-lg border border-border bg-card p-3">
      <div className="mb-2 flex items-center gap-2">
        <span className="text-[0.8rem] font-medium">{sample.name}</span>
        {quality && <span className="text-[0.75rem] text-muted-foreground">{quality.seconds.toFixed(1)} s</span>}
        <Button
          variant="outline"
          size="sm"
          aria-label={`Test voice ${sample.name}`}
          title="Hear this sample read the test text"
          disabled={!canTest || testing}
          className="ml-auto"
          onClick={onTest}
        >
          {testing ? <Loader2 size={14} className="animate-spin" /> : <Volume2 size={14} />}
          {testing ? "Speaking..." : "Test"}
        </Button>
        <Button
          variant="ghost"
          size="icon-xs"
          aria-label={`Delete ${sample.name}`}
          title="Delete sample"
          className="text-muted-foreground hover:text-destructive"
          onClick={onDelete}
        >
          <Trash2 size={14} />
        </Button>
      </div>
      {src && <AudioPlayer src={src} ariaLabel={sample.name} path={sample.path} />}
      {quality?.issues.map((issue) => (
        <p key={issue} className="mt-2 flex items-center gap-1 text-[0.75rem] text-amber-500">
          <AlertTriangle size={12} aria-hidden="true" />
          {ISSUE_ADVICE[issue]}
        </p>
      ))}
      {testSrc && test && (
        <div className="mt-3 border-t border-border pt-3">
          <p className="mb-2 text-[0.75rem] text-muted-foreground">How an agent sounds in this voice</p>
          <AudioPlayer
            key={test.at}
            src={`${testSrc}?at=${test.at}`}
            ariaLabel={`Test of ${sample.name}`}
            path={test.path}
          />
        </div>
      )}
      {testError && (
        <p role="status" className="mt-2 text-[0.75rem] text-err">
          {testError}
        </p>
      )}
    </div>
  );
}

export function VoiceSamplesTab() {
  const [samples, setSamples] = useState<VoiceSample[]>([]);
  const [error, setError] = useState("");
  const [adding, setAdding] = useState(false);
  const [recording, setRecording] = useState(false);
  const [level, setLevel] = useState(0);
  const [speech, setSpeech] = useState<SpeechSetup | null>(null);
  const [testing, setTesting] = useState<string | null>(null);
  const [tests, setTests] = useState<Record<string, { path: string; at: number }>>({});
  const [testErrors, setTestErrors] = useState<Record<string, string>>({});
  const testText = useRef<HTMLTextAreaElement>(null);

  const mediaStream = useRef<MediaStream | null>(null);
  const audioCtx = useRef<AudioContext | null>(null);
  const recNode = useRef<ScriptProcessorNode | null>(null);
  const recChunks = useRef<Float32Array[]>([]);
  const recTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const nameRef = useRef<HTMLInputElement>(null);

  useEffect(
    () => () => {
      if (recTimer.current) clearTimeout(recTimer.current);
      mediaStream.current?.getTracks().forEach((t) => t.stop());
      audioCtx.current?.close();
    },
    [],
  );

  const refresh = useCallback(() => {
    api
      .listVoiceSamples()
      .then(setSamples)
      .catch((e) => setError(String(e)));
  }, []);

  useEffect(refresh, [refresh]);

  useEffect(() => {
    api
      .getConfig()
      .then((c) => setSpeech({ enabled: c.text_to_speech_enabled, model: c.text_to_speech_model }))
      .catch(() => {});
  }, []);

  const canTest = !!speech?.enabled && testing === null;

  const test = async (name: string) => {
    setTesting(name);
    setTestErrors((errors) => ({ ...errors, [name]: "" }));
    try {
      const path = await api.testVoiceSample(name, testText.current?.value.trim() || TEST_READING);
      setTests((done) => ({ ...done, [name]: { path, at: Date.now() } }));
    } catch (e) {
      setTestErrors((errors) => ({ ...errors, [name]: String(e) }));
    } finally {
      setTesting(null);
    }
  };

  const add = async () => {
    setAdding(true);
    setError("");
    try {
      const added = await api.addVoiceSample();
      if (added) refresh();
    } catch (e) {
      setError(String(e));
    } finally {
      setAdding(false);
    }
  };

  const remove = async (name: string) => {
    setError("");
    try {
      await api.deleteVoiceSample(name);
      refresh();
    } catch (e) {
      setError(String(e));
    }
  };

  // Same capture wiring as useVoiceInput, but kept at the native sample rate
  // (no 16kHz downsample) - cloning wants the full-quality reference.
  const stopRecording = useCallback(async () => {
    setRecording(false);
    if (recTimer.current) clearTimeout(recTimer.current);
    if (recNode.current) {
      recNode.current.disconnect();
      recNode.current.onaudioprocess = null;
      recNode.current = null;
    }
    mediaStream.current?.getTracks().forEach((t) => t.stop());
    mediaStream.current = null;
    const rate = audioCtx.current?.sampleRate ?? 48000;
    if (audioCtx.current) {
      await audioCtx.current.close();
      audioCtx.current = null;
    }

    const samples = trimSilence(mergeChunks(recChunks.current), rate);
    recChunks.current = [];
    setLevel(0);
    if (samples.length === 0) {
      setError("No audio captured");
      return;
    }
    const name = nameRef.current?.value.trim() || "my-voice";
    const file = name.toLowerCase().endsWith(".wav") ? name : `${name}.wav`;
    try {
      await api.saveVoiceSample(file, Array.from(encodeWav(samples, rate)));
      refresh();
    } catch (e) {
      setError(String(e));
    }
  }, [refresh]);

  const record = async () => {
    if (recording) {
      await stopRecording();
      return;
    }
    setError("");
    try {
      mediaStream.current = await navigator.mediaDevices.getUserMedia({ audio: RAW_MICROPHONE });
    } catch {
      setError("Microphone access denied");
      return;
    }
    const ctx = newAudioContext();
    audioCtx.current = ctx;
    recChunks.current = [];
    const source = ctx.createMediaStreamSource(mediaStream.current);
    const node = ctx.createScriptProcessor(4096, 1, 1);
    node.onaudioprocess = (e) => {
      const chunk = new Float32Array(e.inputBuffer.getChannelData(0));
      recChunks.current.push(chunk);
      setLevel(peakLevel(chunk));
    };
    source.connect(node);
    node.connect(ctx.destination);
    recNode.current = node;
    setRecording(true);
    recTimer.current = setTimeout(() => void stopRecording(), MAX_SAMPLE_REC_MS);
  };

  return (
    <>
      <h2 className="text-[1.05rem] font-semibold">Voice samples</h2>
      <p className="mb-5 text-[0.8rem] text-muted-foreground">
        Reference WAV recordings for voice cloning, stored in ~/.infer/voice/samples. Ask the agent to speak in a
        sample's voice by its full file name, e.g. "read this in the voice of my-voice.wav".
      </p>
      <div className="mb-4 rounded-lg border border-border bg-card p-3 text-[0.8rem]">
        <p>
          A good reference, recorded or uploaded: a quiet room, a consistent distance from the mic, natural delivery,
          and nothing but the sample text. Aim for 10-13 seconds per sample. {RECOMMENDED_SAMPLES} different ones clone
          best.
        </p>
        <p className="mt-2 text-muted-foreground">
          Read one of these while recording, or before uploading; click a style to use it as the sample name:
        </p>
        <ul className="mt-2 flex flex-col gap-3">
          {SAMPLE_READINGS.map((r) => (
            <li key={r.style}>
              <button
                type="button"
                title={`Use "${r.style.toLowerCase().replaceAll(" ", "-")}" as the sample name`}
                className="font-medium underline decoration-dotted underline-offset-2 hover:text-foreground"
                onClick={() => {
                  if (nameRef.current) nameRef.current.value = r.style.toLowerCase().replaceAll(" ", "-");
                }}
              >
                {r.style}
              </button>
              <span className="ml-2 text-muted-foreground italic">{r.tip}</span>
              <p className="mt-0.5">"{r.text}"</p>
            </li>
          ))}
        </ul>
      </div>
      <div className="mb-4 flex items-center gap-2">
        <Button size="sm" disabled={adding || recording} onClick={add}>
          {adding ? "Adding..." : "Add sample"}
        </Button>
        <Input
          ref={nameRef}
          defaultValue="my-voice"
          aria-label="Sample name"
          placeholder="Sample name"
          className="h-7 w-40 text-[0.8rem]"
        />
        <Button
          size="icon-sm"
          variant={recording ? "destructive" : "outline"}
          aria-label={recording ? "Stop recording" : "Record voice sample"}
          title={recording ? "Stop recording" : "Record a voice sample"}
          disabled={adding}
          onClick={() => void record()}
          className={cn(recording && "mic-recording bg-destructive text-white hover:bg-destructive")}
        >
          {recording ? <Square size={14} /> : <Mic size={14} />}
        </Button>
        {recording && <LevelMeter level={level} />}
        {error && (
          <span role="status" className="text-[0.75rem] text-err">
            {error}
          </span>
        )}
      </div>
      <div className="mb-4">
        <label htmlFor="voice-test-text" className="text-[0.8rem] font-medium">
          Test text
        </label>
        <p className="mb-2 text-[0.75rem] text-muted-foreground">{testHint(speech)}</p>
        <textarea
          id="voice-test-text"
          ref={testText}
          defaultValue={TEST_READING}
          rows={3}
          className="w-full rounded border border-border bg-secondary px-2 py-1.5 text-[0.8rem] text-foreground"
        />
      </div>
      <p className="mb-2 text-[0.8rem] text-muted-foreground">
        {samples.length} of {RECOMMENDED_SAMPLES} recommended samples
        {samples.length >= RECOMMENDED_SAMPLES && (
          <span role="status" className="ml-2 inline-flex items-center gap-1 text-emerald-500">
            <CheckCircle2 size={14} aria-hidden="true" />
            Sample library complete
          </span>
        )}
      </p>
      {samples.length === 0 ? (
        <p className="text-[0.8rem] text-muted-foreground">No voice samples yet.</p>
      ) : (
        <div className="flex flex-col gap-3">
          {samples.map((s) => (
            <SampleCard
              key={s.name}
              sample={s}
              canTest={canTest}
              testing={testing === s.name}
              test={tests[s.name]}
              testError={testErrors[s.name]}
              onTest={() => void test(s.name)}
              onDelete={() => remove(s.name)}
            />
          ))}
        </div>
      )}
    </>
  );
}
