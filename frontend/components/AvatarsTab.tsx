import { useCallback, useEffect, useRef, useState } from "react";
import { Clapperboard, Loader2, Trash2 } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { api, Channel, type Avatar, type DesktopConfig, type VoiceSample } from "@/lib/tauri";
import { AVATAR_TEST_LINE, SETTINGS_VOICE, avatarTestBlocker, defaultTestVoice } from "@/lib/avatars";
import { safeImageSrc, safeVideoSrc } from "@/lib/tools";

const TEST_HINT =
  "Test speaks the line below and lip-syncs the avatar's first image to it, the way an agent does. Every test renders a clip at the video provider, so keep the line short.";

type TestClip = { path: string; at: number };

function AvatarCard({
  avatar,
  canTest,
  testing,
  clip,
  testError,
  onTest,
  onDelete,
}: {
  avatar: Avatar;
  canTest: boolean;
  testing: boolean;
  clip?: TestClip;
  testError?: string;
  onTest: () => void;
  onDelete: () => void;
}) {
  const clipSrc = safeVideoSrc(clip?.path);
  return (
    <div className="rounded-lg border border-border bg-card p-3">
      <div className="mb-2 flex items-center gap-2">
        <span className="text-[0.8rem] font-medium">{avatar.name}</span>
        <Button
          variant="outline"
          size="sm"
          aria-label={`Test avatar ${avatar.name}`}
          title="See this avatar say the test line"
          disabled={!canTest || testing}
          className="ml-auto"
          onClick={onTest}
        >
          {testing ? <Loader2 size={14} className="animate-spin" /> : <Clapperboard size={14} />}
          {testing ? "Rendering..." : "Test"}
        </Button>
        <Button
          variant="ghost"
          size="icon-xs"
          aria-label={`Delete avatar ${avatar.name}`}
          title="Delete avatar"
          className="text-muted-foreground hover:text-destructive"
          onClick={onDelete}
        >
          <Trash2 size={14} />
        </Button>
      </div>
      <div className="flex flex-wrap gap-2">
        {avatar.images.map((img) => {
          const src = safeImageSrc(img);
          return (
            src && (
              <img
                key={img}
                src={src}
                alt={img.split(/[\\/]/).pop()}
                className="h-32 rounded-md border border-border object-cover"
              />
            )
          );
        })}
      </div>
      {testing && (
        <p role="status" className="mt-2 text-[0.75rem] text-muted-foreground">
          Speaking the line and rendering the clip - this can take a few minutes.
        </p>
      )}
      {clipSrc && clip && (
        <div className="mt-3 border-t border-border pt-3">
          <p className="mb-2 text-[0.75rem] text-muted-foreground">How an agent looks and sounds as this avatar</p>
          <video
            key={clip.at}
            controls
            playsInline
            src={`${clipSrc}?at=${clip.at}`}
            aria-label={`Test of ${avatar.name}`}
            className="max-h-80 rounded-md border border-border"
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

// Avatar library for the CLI's TextToVideo tool, backed by the avatar commands
// (backend/src/avatars.rs) that shell out to `infer avatars`. The photo comes
// from a native picker or a camera snapshot; previews and test clips stream
// through the asset protocol.
export function AvatarsTab() {
  const [avatars, setAvatars] = useState<Avatar[]>([]);
  const [setup, setSetup] = useState<DesktopConfig | null>(null);
  const [samples, setSamples] = useState<VoiceSample[]>([]);
  const [pickedVoice, setPickedVoice] = useState<string | null>(null);
  const [testing, setTesting] = useState<string | null>(null);
  const [clips, setClips] = useState<Record<string, TestClip>>({});
  const [testErrors, setTestErrors] = useState<Record<string, string>>({});
  const [error, setError] = useState("");
  const [progress, setProgress] = useState("");
  const [busy, setBusy] = useState(false);
  const [camera, setCamera] = useState(false);

  const stream = useRef<MediaStream | null>(null);
  const video = useRef<HTMLVideoElement>(null);
  const nameRef = useRef<HTMLInputElement>(null);
  const testLine = useRef<HTMLTextAreaElement>(null);

  useEffect(() => () => stream.current?.getTracks().forEach((t) => t.stop()), []);

  useEffect(() => {
    api
      .getConfig()
      .then(setSetup)
      .catch((e) => setError(String(e)));
    api
      .listVoiceSamples()
      .then(setSamples)
      .catch(() => setSamples([]));
  }, []);

  const blocker = avatarTestBlocker(setup);
  const voice = pickedVoice ?? defaultTestVoice(setup?.text_to_speech_voice ?? "", samples);

  const test = async (name: string) => {
    setTesting(name);
    setTestErrors((errors) => ({ ...errors, [name]: "" }));
    try {
      const line = testLine.current?.value.trim() || AVATAR_TEST_LINE;
      const path = await api.testAvatar(name, line, voice === SETTINGS_VOICE ? null : voice);
      setClips((done) => ({ ...done, [name]: { path, at: Date.now() } }));
    } catch (e) {
      setTestErrors((errors) => ({ ...errors, [name]: String(e) }));
    } finally {
      setTesting(null);
    }
  };

  useEffect(() => {
    if (camera && video.current) video.current.srcObject = stream.current;
  }, [camera]);

  const refresh = useCallback(() => {
    api
      .listAvatars()
      .then(setAvatars)
      .catch((e) => setError(String(e)));
  }, []);

  useEffect(refresh, [refresh]);

  const create = async (run: (name: string, onLine: Channel<string>) => Promise<unknown>) => {
    setBusy(true);
    setError("");
    setProgress("");
    const onLine = new Channel<string>();
    onLine.onmessage = setProgress;
    try {
      await run(nameRef.current?.value.trim() ?? "", onLine);
      refresh();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
      setProgress("");
    }
  };

  const stopCamera = () => {
    stream.current?.getTracks().forEach((t) => t.stop());
    stream.current = null;
    setCamera(false);
  };

  const openCamera = async () => {
    setError("");
    try {
      stream.current = await navigator.mediaDevices.getUserMedia({ video: true });
      setCamera(true);
    } catch (e) {
      setError(`Camera unavailable: ${e}`);
    }
  };

  const capture = async () => {
    const v = video.current;
    if (!v) return;
    const canvas = document.createElement("canvas");
    canvas.width = v.videoWidth;
    canvas.height = v.videoHeight;
    canvas.getContext("2d")?.drawImage(v, 0, 0);
    const blob = await new Promise<Blob | null>((resolve) => canvas.toBlob(resolve, "image/jpeg", 0.92));
    stopCamera();
    if (!blob) {
      setError("Could not capture a photo from the camera");
      return;
    }
    const jpeg = Array.from(new Uint8Array(await blob.arrayBuffer()));
    await create((name, onLine) => api.snapshotAvatar(name, jpeg, onLine));
  };

  const remove = async (name: string) => {
    setError("");
    try {
      await api.deleteAvatar(name);
      refresh();
    } catch (e) {
      setError(String(e));
    }
  };

  return (
    <>
      <h2 className="text-[1.05rem] font-semibold">Avatars</h2>
      <p className="mb-5 text-[0.8rem] text-muted-foreground">
        Portraits the agent turns into lip-synced talking clips in Content projects, stored in
        ~/.infer/avatars/&lt;name&gt;. Creating one keeps your front-facing photo and generates two three-quarter views
        from it; the first image is the one that talks.
      </p>
      <div className="mb-4 rounded-lg border border-border bg-card p-3 text-[0.8rem]">
        <p>
          Use only your own likeness or one you have rights to. The photo goes to the image-edit provider when the extra
          angles are generated, and the portrait and voice clip go to the video provider on every render.
        </p>
        <p className="mt-2 text-muted-foreground">
          Talking clips render only while Text to video is on in Settings &gt; General - it is off by default.
        </p>
      </div>
      <div className="mb-4 flex items-center gap-2">
        <Input
          ref={nameRef}
          defaultValue="presenter"
          aria-label="Avatar name"
          placeholder="Avatar name"
          className="h-7 w-40 text-[0.8rem]"
        />
        <Button size="sm" disabled={busy || camera} onClick={() => void create(api.importAvatar)}>
          Import photo
        </Button>
        <Button size="sm" variant="outline" disabled={busy} onClick={() => (camera ? stopCamera() : void openCamera())}>
          {camera ? "Close camera" : "Take photo"}
        </Button>
        {error && (
          <span role="status" className="text-[0.75rem] text-err">
            {error}
          </span>
        )}
      </div>
      {camera && (
        <div className="mb-4 flex flex-col items-start gap-2">
          <video ref={video} autoPlay muted playsInline className="w-80 -scale-x-100 rounded-lg border border-border" />
          <Button size="sm" onClick={() => void capture()}>
            Capture
          </Button>
        </div>
      )}
      {busy && (
        <p role="status" className="mb-4 text-[0.8rem] text-muted-foreground">
          {progress || "Creating avatar - generating the extra views can take a minute..."}
        </p>
      )}
      <div className="mb-4">
        <label htmlFor="avatar-test-text" className="text-[0.8rem] font-medium">
          Test line
        </label>
        <p className="mb-2 text-[0.75rem] text-muted-foreground">{blocker || TEST_HINT}</p>
        <textarea
          id="avatar-test-text"
          ref={testLine}
          defaultValue={AVATAR_TEST_LINE}
          rows={2}
          className="w-full rounded border border-border bg-secondary px-2 py-1.5 text-[0.8rem] text-foreground"
        />
        <div className="mt-2 flex items-center gap-2">
          <label htmlFor="avatar-test-voice" className="text-[0.8rem] font-medium">
            Voice
          </label>
          <select
            id="avatar-test-voice"
            value={voice}
            onChange={(e) => setPickedVoice(e.target.value)}
            className="max-w-sm rounded border border-border bg-secondary px-2 py-1.5 text-[0.8rem] text-foreground"
          >
            <option value={SETTINGS_VOICE}>The voice of the speech settings</option>
            {samples.map((s) => (
              <option key={s.name} value={s.name}>
                {s.name}
              </option>
            ))}
          </select>
        </div>
      </div>
      {avatars.length === 0 ? (
        <p className="text-[0.8rem] text-muted-foreground">No avatars yet.</p>
      ) : (
        <div className="flex flex-col gap-3">
          {avatars.map((a) => (
            <AvatarCard
              key={a.name}
              avatar={a}
              canTest={blocker === "" && testing === null}
              testing={testing === a.name}
              clip={clips[a.name]}
              testError={testErrors[a.name]}
              onTest={() => void test(a.name)}
              onDelete={() => void remove(a.name)}
            />
          ))}
        </div>
      )}
    </>
  );
}
