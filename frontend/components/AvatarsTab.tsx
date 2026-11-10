import { useCallback, useEffect, useRef, useState } from "react";
import { Trash2 } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { api, Channel, type Avatar } from "@/lib/tauri";
import { safeImageSrc } from "@/lib/tools";

// Avatar library for the CLI's TextToVideo tool, backed by the avatar commands
// (backend/src/avatars.rs) that shell out to `infer avatars`. The photo comes
// from a native picker or a camera snapshot; previews stream through the asset
// protocol.
export function AvatarsTab() {
  const [avatars, setAvatars] = useState<Avatar[]>([]);
  const [error, setError] = useState("");
  const [progress, setProgress] = useState("");
  const [busy, setBusy] = useState(false);
  const [camera, setCamera] = useState(false);

  const stream = useRef<MediaStream | null>(null);
  const video = useRef<HTMLVideoElement>(null);
  const nameRef = useRef<HTMLInputElement>(null);

  useEffect(() => () => stream.current?.getTracks().forEach((t) => t.stop()), []);

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
      {avatars.length === 0 ? (
        <p className="text-[0.8rem] text-muted-foreground">No avatars yet.</p>
      ) : (
        <div className="flex flex-col gap-3">
          {avatars.map((a) => (
            <div key={a.name} className="rounded-lg border border-border bg-card p-3">
              <div className="mb-2 flex items-center gap-2">
                <span className="text-[0.8rem] font-medium">{a.name}</span>
                <Button
                  variant="ghost"
                  size="icon-xs"
                  aria-label={`Delete avatar ${a.name}`}
                  title="Delete avatar"
                  className="ml-auto text-muted-foreground hover:text-destructive"
                  onClick={() => void remove(a.name)}
                >
                  <Trash2 size={14} />
                </Button>
              </div>
              <div className="flex flex-wrap gap-2">
                {a.images.map((img) => {
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
            </div>
          ))}
        </div>
      )}
    </>
  );
}
