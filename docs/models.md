# Models (Hugging Face downloads)

Settings > **Models** searches the Hugging Face Hub and keeps model files on this computer. The app runs `infer models`, so the CLI owns the search, the downloads and the library.

- **Search** lists matching models, the most downloaded first.
- **Files** on a result shows every file of the model with its size. All of them are picked to begin with. Untick the ones you do not need, which matters for a model that ships many variants.
- **Download** fetches the picked files and shows the progress. A file that is already there is kept, so a download that was stopped picks up where it left off, file by file.
- **Downloaded** lists what is on disk, with a delete button per model.

Each model is a folder that keeps the layout the Hub gives it:

```text
~/.infer/models/huggingface/
  mlx-community/
    MuseTalk-1.5-fp16/
      config.json
      unet.safetensors
```

Only public models can be downloaded. A download gives you the files and nothing more - check the licence on the model's Hub page before you use it, and note that nothing in the app runs a downloaded model by itself yet.
