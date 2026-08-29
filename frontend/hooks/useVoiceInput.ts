import { useCallback, useEffect, useRef, useState } from "react";
import type { RefObject } from "react";
import { api, Channel, type ProgressEvent, type SttStatus } from "@/lib/tauri";
import { downsample, encodeWav, mergeChunks } from "@/lib/audio";
import { autoGrow } from "@/lib/textarea";

// Voice-to-text: capture in the WebView, resample to 16kHz mono WAV, hand to the
// Rust transcribe_audio command (whisper.cpp). Mic is greyed when whisper is
// unavailable, permission is denied, or the agent is running.
const MAX_REC_MS = 30000;

type Options = {
  textareaRef: RefObject<HTMLTextAreaElement | null>;
  running: boolean;
  setStatus: (text: string) => void;
  setError: (text: string) => void;
};

export type VoiceInput = {
  recording: boolean;
  disabled: boolean;
  title: string;
  onClick: () => void;
};

export function useVoiceInput({ textareaRef, running, setStatus, setError }: Options): VoiceInput {
  const [sttBinary, setSttBinary] = useState(false);
  const [sttModel, setSttModel] = useState(false);
  const [sttDownloadable, setSttDownloadable] = useState(false);
  const [sttHint, setSttHint] = useState("");
  const [micPermission, setMicPermission] = useState<string>("prompt");
  const [recording, setRecording] = useState(false);

  const recordingRef = useRef(false);
  const preparingRef = useRef(false);
  const mediaStream = useRef<MediaStream | null>(null);
  const audioCtx = useRef<AudioContext | null>(null);
  const recNode = useRef<ScriptProcessorNode | null>(null);
  const recChunks = useRef<Float32Array[]>([]);
  const recSampleRate = useRef(48000);
  const recTimer = useRef<ReturnType<typeof setTimeout> | null>(null);

  const refreshSttStatus = useCallback(async (): Promise<SttStatus | null> => {
    try {
      const s = await api.sttStatus();
      setSttBinary(s.binary);
      setSttModel(s.model);
      setSttDownloadable(s.downloadable);
      setSttHint(s.hint);
      return s;
    } catch {
      setSttBinary(false);
      setSttModel(false);
      return null;
    }
  }, []);

  useEffect(() => {
    refreshSttStatus();
    (async () => {
      const perms = navigator.permissions;
      if (!perms?.query) return;
      try {
        const st = await perms.query({ name: "microphone" as PermissionName });
        setMicPermission(st.state);
        st.onchange = () => setMicPermission(st.state);
      } catch {
        // WebKit may not expose the microphone permission - fall back to prompt-on-use.
      }
    })();
    return () => {
      if (recTimer.current) clearTimeout(recTimer.current);
      mediaStream.current?.getTracks().forEach((t) => t.stop());
      audioCtx.current?.close();
    };
  }, [refreshSttStatus]);

  const ensureStt = useCallback(async () => {
    const ch = new Channel<ProgressEvent>();
    ch.onmessage = (e) => {
      switch (e.kind) {
        case "Checking":
          setStatus("Preparing voice input...");
          break;
        case "Installing":
          setStatus("Installing whisper...");
          break;
        case "Downloading":
          setStatus(
            e.total > 0
              ? `Downloading voice model... ${Math.round((e.received / e.total) * 100)}%`
              : "Downloading voice model...",
          );
          break;
        case "Verifying":
          setStatus("Verifying...");
          break;
      }
    };
    await api.prepareStt(ch);
  }, [setStatus]);

  const stopRecording = useCallback(async () => {
    if (!recordingRef.current) return;
    recordingRef.current = false;
    setRecording(false);
    if (recTimer.current) clearTimeout(recTimer.current);
    if (recNode.current) {
      recNode.current.disconnect();
      recNode.current.onaudioprocess = null;
      recNode.current = null;
    }
    mediaStream.current?.getTracks().forEach((t) => t.stop());
    mediaStream.current = null;
    const ctxRate = recSampleRate.current;
    if (audioCtx.current) {
      await audioCtx.current.close();
      audioCtx.current = null;
    }

    const samples = mergeChunks(recChunks.current);
    recChunks.current = [];
    if (samples.length === 0) {
      setStatus("No audio captured");
      return;
    }
    const wav = encodeWav(downsample(samples, ctxRate, 16000), 16000);

    setStatus("Transcribing...");
    try {
      const text = await api.transcribeAudio(Array.from(wav));
      const el = textareaRef.current;
      if (text && text.trim()) {
        if (el) {
          el.value = el.value ? el.value.trimEnd() + " " + text.trim() : text.trim();
          autoGrow(el);
          el.focus();
        }
        setStatus("Ready");
      } else {
        setStatus("No speech detected");
      }
    } catch (err) {
      setError(`Transcription failed: ${err}`);
    }
  }, [setStatus, setError, textareaRef]);

  const startRecording = useCallback(async () => {
    try {
      mediaStream.current = await navigator.mediaDevices.getUserMedia({ audio: true });
    } catch {
      setMicPermission("denied");
      setError("Microphone access denied");
      return;
    }
    setMicPermission("granted");
    const Ctx: typeof AudioContext =
      window.AudioContext || (window as unknown as { webkitAudioContext: typeof AudioContext }).webkitAudioContext;
    const ctx = new Ctx();
    audioCtx.current = ctx;
    recSampleRate.current = ctx.sampleRate;
    recChunks.current = [];
    const source = ctx.createMediaStreamSource(mediaStream.current);
    const node = ctx.createScriptProcessor(4096, 1, 1);
    node.onaudioprocess = (e) => {
      recChunks.current.push(new Float32Array(e.inputBuffer.getChannelData(0)));
    };
    source.connect(node);
    node.connect(ctx.destination);
    recNode.current = node;
    recordingRef.current = true;
    setRecording(true);
    setStatus("Recording... click the mic to stop");
    recTimer.current = setTimeout(stopRecording, MAX_REC_MS);
  }, [setStatus, setError, stopRecording]);

  const onClick = useCallback(async () => {
    if (recordingRef.current) {
      await stopRecording();
      return;
    }
    if (running || preparingRef.current) return;
    if (!(sttBinary && sttModel)) {
      if (!window.confirm("Download voice support (~75 MB)? One-time setup.")) return;
      preparingRef.current = true;
      let fresh: SttStatus | null;
      try {
        await ensureStt();
        fresh = await refreshSttStatus();
      } catch (err) {
        setError(`Voice setup failed: ${err}`);
        await refreshSttStatus();
        return;
      } finally {
        preparingRef.current = false;
      }
      if (!(fresh && fresh.binary && fresh.model)) return;
    }
    await startRecording();
  }, [running, sttBinary, sttModel, ensureStt, refreshSttStatus, startRecording, stopRecording, setError]);

  const denied = micPermission === "denied";
  const ready = sttBinary && sttModel;
  const canPrepare = sttBinary || sttDownloadable;
  const disabled = recording ? false : running || denied || !(ready || canPrepare);
  const title = running
    ? "Wait for the current message to finish"
    : denied
      ? "Microphone access denied - enable it in System Settings"
      : !canPrepare
        ? sttHint || "Voice input unavailable"
        : recording
          ? "Stop recording"
          : !ready
            ? "Click to set up voice input (downloads once)"
            : "Voice input";

  return { recording, disabled, title, onClick: () => void onClick() };
}
