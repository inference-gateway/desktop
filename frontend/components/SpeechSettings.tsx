import { useEffect, useRef, useState, type FocusEvent, type KeyboardEvent } from "react";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { api, type DesktopConfig } from "@/lib/tauri";
import {
  effectiveSpeechModel,
  isLocalSpeechModel,
  keepsVoiceSamples,
  speechModelOptions,
  speechRestartsGateway,
} from "@/lib/speech-models";

type SpeechChange = Partial<DesktopConfig>;

export type SpeechConfig = {
  config: DesktopConfig | null;
  status: string;
  error: string;
  save: (change: SpeechChange) => void;
};

/**
 * The CLI's text_to_speech settings, saved as soon as one changes, so a voice
 * test always speaks with what the tab shows. Saves run one after another and
 * each starts from the config on disk, so none overwrites another.
 */
export function useSpeechConfig(): SpeechConfig {
  const [config, setConfig] = useState<DesktopConfig | null>(null);
  const [status, setStatus] = useState("");
  const [error, setError] = useState("");
  const queue = useRef<Promise<void>>(Promise.resolve());

  useEffect(() => {
    api
      .getConfig()
      .then(setConfig)
      .catch((e) => setError(String(e)));
  }, []);

  const write = async (change: SpeechChange) => {
    setError("");
    setStatus("Saving...");
    try {
      const before = await api.getConfig();
      const next = { ...before, ...change };
      await api.setConfig(next);
      setConfig(next);
      if (speechRestartsGateway(before, next)) {
        setStatus("Restarting the gateway...");
        await api.startGateway(false, true);
      }
      setStatus("Saved");
    } catch (e) {
      setStatus("");
      setError(`Couldn't save: ${e}`);
      setConfig(await api.getConfig().catch(() => null));
    }
  };

  const save = (change: SpeechChange) => {
    setConfig((current) => current && { ...current, ...change });
    queue.current = queue.current.then(() => write(change));
  };

  return { config, status, error, save };
}

const ROW_LABEL = "w-28 shrink-0 text-[0.8rem] font-medium";
const HINT = "text-[0.75rem] text-muted-foreground";

function blurOnEnter(e: KeyboardEvent<HTMLInputElement>) {
  if (e.key === "Enter") e.currentTarget.blur();
}

export function SpeechSettings({ config, status, error, save }: SpeechConfig) {
  if (!config) {
    return error ? (
      <p role="status" className="mb-5 text-[0.8rem] text-err">
        {error}
      </p>
    ) : null;
  }

  const local = isLocalSpeechModel(config.text_to_speech_model);
  const keepsSamples = keepsVoiceSamples(config.text_to_speech_model);
  const saveText = (key: "text_to_speech_voice" | "text_to_speech_output_dir") => (e: FocusEvent<HTMLInputElement>) => {
    const value = e.target.value.trim();
    if (value !== config[key]) save({ [key]: value });
  };

  return (
    <section
      aria-label="Text to speech"
      className="mb-5 flex flex-col gap-3 rounded-lg border border-border bg-card p-3"
    >
      <div className="flex items-center gap-3">
        <input
          type="checkbox"
          id="tts-enabled"
          checked={config.text_to_speech_enabled}
          onChange={(e) => save({ text_to_speech_enabled: e.target.checked })}
          className="h-4 w-4 accent-primary"
        />
        <Label htmlFor="tts-enabled" className="cursor-pointer text-[0.8rem] font-medium">
          Enable Text to Speech
        </Label>
        <span role="status" className={`ml-auto text-[0.75rem] ${error ? "text-err" : "text-muted-foreground"}`}>
          {error || status}
        </span>
      </div>
      <p className={HINT}>
        Lets the agent speak with the CLI's TextToSpeech tool. Changes save as you make them, and turning speech on or
        off restarts the gateway.
      </p>

      <div className="flex items-center gap-3">
        <Label htmlFor="tts-model" className={ROW_LABEL}>
          Speech model
        </Label>
        <select
          id="tts-model"
          value={effectiveSpeechModel(config.text_to_speech_model)}
          onChange={(e) => save({ text_to_speech_model: e.target.value })}
          className="w-full max-w-sm rounded border border-border bg-secondary px-2 py-1.5 text-[0.85rem] text-foreground"
        >
          {speechModelOptions(config.text_to_speech_model).map((m) => (
            <option key={m.id} value={m.id}>
              {m.label}
            </option>
          ))}
        </select>
      </div>

      {local ? (
        <>
          <p className={HINT}>Runs on this computer and clones a voice sample when the agent names one.</p>
          <div className="flex items-center gap-3">
            <input
              type="checkbox"
              id="tts-auto-download"
              checked={config.text_to_speech_auto_download}
              onChange={(e) => save({ text_to_speech_auto_download: e.target.checked })}
              className="h-4 w-4 accent-primary"
            />
            <Label htmlFor="tts-auto-download" className="cursor-pointer text-[0.8rem] font-medium">
              Download the local model when it is missing
            </Label>
            <span className={HINT}>About 1.4 GB, on first use.</span>
          </div>
        </>
      ) : (
        <>
          <div className="flex items-center gap-3">
            <Label htmlFor="tts-voice" className={ROW_LABEL}>
              Voice
            </Label>
            <Input
              id="tts-voice"
              key={config.text_to_speech_voice}
              defaultValue={config.text_to_speech_voice}
              onBlur={saveText("text_to_speech_voice")}
              onKeyDown={blurOnEnter}
              placeholder={
                keepsSamples ? "Optional - a voice id for speech without a sample" : "Voice id from your provider"
              }
              className="max-w-sm"
            />
          </div>
          {keepsSamples ? (
            <p className={HINT}>
              Sends the text to ElevenLabs and needs its key under API Keys. A voice sample the agent names is cloned
              into your ElevenLabs account the first time it is used, and stays there until you delete it. A restricted
              key needs the Voices permission, read and write, as well as Text to Speech. Without a sample it speaks in
              this voice.
            </p>
          ) : (
            <p className={HINT}>
              A hosted model sends the text to its provider and needs that provider's key under API Keys. It speaks in
              this voice and does not clone voice samples.
            </p>
          )}
        </>
      )}

      <div className="flex items-center gap-3">
        <Label htmlFor="tts-output-dir" className={ROW_LABEL}>
          Save speech to
        </Label>
        <Input
          id="tts-output-dir"
          key={config.text_to_speech_output_dir}
          defaultValue={config.text_to_speech_output_dir}
          onBlur={saveText("text_to_speech_output_dir")}
          onKeyDown={blurOnEnter}
          placeholder="default: ~/.infer/tmp/media/tts"
          className="max-w-sm"
        />
      </div>
      {config.text_to_speech_output_dir && (
        <p className={HINT}>
          The chat only plays speech saved in the default folder - leave this empty to hear it there.
        </p>
      )}

      <div className="flex items-center gap-3">
        <input
          type="checkbox"
          id="tts-require-approval"
          checked={config.text_to_speech_require_approval}
          onChange={(e) => save({ text_to_speech_require_approval: e.target.checked })}
          className="h-4 w-4 accent-primary"
        />
        <Label htmlFor="tts-require-approval" className="cursor-pointer text-[0.8rem] font-medium">
          Ask before the agent speaks
        </Label>
      </div>
    </section>
  );
}
