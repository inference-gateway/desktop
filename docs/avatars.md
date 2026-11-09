# Avatars (talking clips)

Settings > **Avatars** keeps named portraits of you that the agent turns into lip-synced talking clips in Content projects. Import a front-facing photo or take one with the camera. The app runs `infer avatars create <name> --from <photo>`, which keeps the photo and generates two three-quarter views of it through the gateway's image edit API.

Then turn on **Text to video** in Settings > General (off by default) and ask for a talking avatar, presenter or talking head in a Content project. The bundled `video-editing` skill does the rest:

- It writes the script and voice as spoken clips with `TextToSpeech`.
- It renders each clip with the CLI's `TextToVideo` tool, passing `avatar` and `audio`.
- It places each rendered clip on the video track over its spoken clip, marked with `avatar`. On the timeline the clip is ordinary footage labelled with the avatar's name, and Export needs nothing extra.

Each avatar is a folder of portraits of one person, and the CLI owns the library (`infer avatars list` / `delete`):

```text
~/.infer/avatars/
  presenter/
    01-front.jpeg              # the first image in sort order is the one that talks
    02-three-quarter-left.png
    03-three-quarter-right.png
```

Use only your own likeness or one you have rights to. The photo is sent to the image edit provider (`tools.image_edit.model`, OpenAI by default) when the extra views are generated. The portrait and the voice clip are sent to the video provider on every render. Nothing is stored as an avatar at the provider.

Renders go through the gateway's Videos API with ElevenLabs `creatify-aurora`, so add an ElevenLabs key under Settings > API Keys. Saving the toggle restarts the gateway with `VIDEOS_ENABLED=true`.
