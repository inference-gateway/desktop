---
name: motion-cards
description: Add animated cards to a content project's video - title cards, lower thirds, callouts, numbered steps and bar charts written as HTML/CSS, rendered to a transparent ProRes 4444 MOV with HyperFrames and placed on an overlay track of <stem>.timeline.json so the desktop composites them over the recording. Use when the user asks for a title, intro card, lower third, caption, callout, arrow, label, step counter, chart or any on-screen graphic on a video.
license: Apache-2.0
---

# Motion Cards

Use this skill when the user wants a graphic layered over a video in a content project: a title,
a lower third, a callout pointing at something on screen, a numbered step or an animated chart.
Cards are HTML files you write, rendered by HyperFrames into a `.mov` (ProRes 4444, alpha channel), and
placed as clips on an `overlay` track of `<stem>.timeline.json`. The desktop shows the card over
the video in its preview and composites it into the export; you never mux.

Read `~/.infer/skills/video-editing/SKILL.md` first: it owns the timeline contract, the media
folder rules and the ffmpeg probe. This skill only adds the overlay track and the render step.

## Tools you may use

`Bash` for `node`, `npx hyperframes`, `ffmpeg`, `mkdir`, `ls`, `cp` and `rm` (scratch only) inside
the working directory; `Read` and `Write` for the card HTML and the timeline JSON. Do not use
`WebFetch`, `WebSearch`, `find`, `npm install`, `brew`, or any other binary, and do not look for
tools or skills on disk.

## Prerequisites

Check them in this order and stop at the first one missing, telling the user exactly what it is:

1. `node --version` prints v22 or newer. If not: "Node.js 22+ is needed to render cards; install it
   from nodejs.org or `brew install node`, then ask again."
2. `npx --yes hyperframes browser ensure` finds or downloads the Chromium HyperFrames renders with.
   The first run downloads HyperFrames and Chromium, which can take a few minutes; that is expected.
   If it fails, show the user its last lines.
3. `ffmpeg -hide_banner -filters | grep -c ' overlay '` prints `1`. If `0`: "the installed ffmpeg has no
   `overlay` filter; install a full build (`brew install ffmpeg`)".

## Overlay contract (`<stem>.timeline.json`)

```json
{
  "id": "cards",
  "kind": "overlay",
  "clips": [
    {
      "id": "o1",
      "src": "media/o1-title.mov",
      "html": "cards/o1-title.html",
      "start": 0,
      "end": 3
    },
    {
      "id": "o2",
      "src": "media/o2-lower-third.mov",
      "html": "cards/o2-lower-third.html",
      "start": 4.5,
      "end": 9,
      "x": 0.05,
      "y": 0.78,
      "width": 0.4
    }
  ]
}
```

- `src` is the rendered card under `media/`; `html` is the composition it came from under `cards/`.
  Keep both so a card can be re-rendered instead of rewritten. Never put anything else on an
  overlay track.
- `start`/`end` are seconds on the video; the card plays from its first frame at `start` and is cut
  at `end`. Render each card exactly `end - start` seconds long (its `data-duration`).
- `x`, `y`, `width`, `height` are fractions of the export frame (0-1), top-left origin. Omit all four
  for a full-frame card rendered at the frame's `resolution`; it is scaled to the frame width. Set
  them only for a card rendered smaller than the frame (a lower third, a callout, a badge); `width`
  alone keeps the aspect ratio.
- Use one overlay track with `id: "cards"` unless cards must overlap in time; then add
  `"cards2"`. Clips on one track must not overlap. Clip ids stay stable; the user can move and trim
  cards on the timeline, so read the file back before changing it.

## Steps

1. **Frame size.** The export frame is the timeline's `resolution` (`"WxH"`: `1920x1080` by default, `1080x1920` or `1350x1350`;
   the user picks it on the timeline, so never change it). The recording is scaled to fit and padded
   into that frame, and cards are placed in it. Full-frame cards use exactly that size; smaller
   cards use a size that keeps the same pixel density (a 1920-wide frame with `width: 0.4` means a
   768 px wide card).
2. **Write the card.** `mkdir -p cards media`, then `Write` `cards/<id>-<kind>.html` from the catalogue
   below, changing only the text, the numbers and the tokens block. Keep `data-no-timeline` on the
   root (the cards animate with CSS, there is no GSAP timeline to wait for) and set
   `data-width`/`data-height` and the body size to the card's pixel size, `data-duration` and the
   clip's `data-duration` to `end - start`.
3. **Render.** `npx --yes hyperframes render -c cards/<id>-<kind>.html --format mov -o media/<id>-<kind>.mov --quiet`.
   It writes a ProRes 4444 MOV with alpha, the one format both the desktop's preview and its
   export play transparently (the preview drops the alpha of a WebM, so never use `--format webm`,
   and never mp4: it has no alpha and would cover the video). Check with
   `ffmpeg -hide_banner -i media/<id>-<kind>.mov` that the duration matches; render again after fixing
   the HTML if it does not. ProRes files are large (roughly 10 MB per second of full HD), which is
   fine for a handful of short cards; keep cards under 10 s.
4. **Place it.** Read the timeline, add the clip to the overlay track (create the track if missing),
   write the file back. Repeat from step 2 for the next card.
5. **Stop.** Do not run ffmpeg on the video, do not export: the user previews the cards on the
   timeline and presses Export, which writes `export/<output>`. Say which cards you placed and that each can be moved, trimmed or
   deleted on the timeline, and that asking for a change to a card re-renders just that one.

## Re-rendering

When the user asks to change a card ("make the title say X", "move the callout"), edit the file
named by the clip's `html`, render it to the same `src`, and leave `start`/`end` alone unless asked.
Placement changes (`x`, `y`, `width`) need no render.

## Design tokens

Every template starts with the same tokens so cards match each other and the desktop's look. Change
them only when the user asks for brand colours or a font, and change them in every card of the
project the same way.

```css
:root {
  --font: "Inter", -apple-system, "SF Pro Text", "Segoe UI", system-ui, sans-serif;
  --ink: #fafafa;
  --muted: #a1a1aa;
  --accent: #6366f1;
  --panel: rgba(9, 9, 11, 0.85);
  --radius: 16px;
  --ease: cubic-bezier(0.2, 0.8, 0.2, 1);
}
```

Sizing rules: type is sized for a 1080 px tall frame (headline 64-72 px, body 26-30 px, labels 22 px);
scale every size by `frame_height / 1080` for other frames. Keep at least 48 px of clear frame around
a full-frame card's content. Enter animations take 0.5-0.7 s, exits 0.3-0.4 s, and every element
uses `animation-fill-mode: both` so the first and last frames are stable. Only CSS animations and
transitions: no JavaScript-driven motion, no external scripts, no web fonts fetched from the
network.

## Catalogue

Each template is a complete file. The `<body>` size, `data-width`, `data-height` and the two
`data-duration` values are the only structural fields to change.

### title (full frame)

```html
<!doctype html>
<html lang="en">
  <head>
    <meta charset="UTF-8" />
    <style>
      :root { --font: "Inter", -apple-system, "SF Pro Text", "Segoe UI", system-ui, sans-serif; --ink: #fafafa; --muted: #a1a1aa; --accent: #6366f1; --panel: rgba(9, 9, 11, 0.85); --radius: 16px; --ease: cubic-bezier(0.2, 0.8, 0.2, 1); }
      * { margin: 0; padding: 0; box-sizing: border-box; }
      html, body { width: 1920px; height: 1080px; overflow: hidden; background: transparent; font-family: var(--font); }
      #root { width: 100%; height: 100%; display: flex; align-items: center; justify-content: center; }
      .card { padding: 40px 64px; border-radius: var(--radius); background: var(--panel); color: var(--ink); text-align: center; animation: in 0.6s var(--ease) both, out 0.35s ease-in both 2.65s; }
      h1 { font-size: 72px; font-weight: 700; letter-spacing: -0.02em; }
      p { margin-top: 14px; font-size: 30px; color: var(--muted); }
      .bar { width: 0; height: 6px; margin: 22px auto 0; border-radius: 3px; background: var(--accent); animation: grow 0.5s var(--ease) 0.35s both; }
      @keyframes in { from { opacity: 0; transform: translateY(28px) scale(0.96); } to { opacity: 1; transform: none; } }
      @keyframes out { to { opacity: 0; transform: translateY(-12px); } }
      @keyframes grow { to { width: 120px; } }
    </style>
  </head>
  <body>
    <div id="root" data-composition-id="title" data-no-timeline data-start="0" data-duration="3" data-width="1920" data-height="1080">
      <div class="card clip" data-start="0" data-duration="3" data-track-index="0">
        <h1>Setting up the gateway</h1>
        <p>From install to first request</p>
        <div class="bar"></div>
      </div>
    </div>
  </body>
</html>
```

The `out` animation's delay is `duration - 0.35`; change both when the clip length changes.

### lower-third (`x: 0.05, y: 0.78, width: 0.4`)

```html
<!doctype html>
<html lang="en">
  <head>
    <meta charset="UTF-8" />
    <style>
      :root { --font: "Inter", -apple-system, "SF Pro Text", "Segoe UI", system-ui, sans-serif; --ink: #fafafa; --muted: #a1a1aa; --accent: #6366f1; --panel: rgba(9, 9, 11, 0.85); --radius: 16px; --ease: cubic-bezier(0.2, 0.8, 0.2, 1); }
      * { margin: 0; padding: 0; box-sizing: border-box; }
      html, body { width: 768px; height: 160px; overflow: hidden; background: transparent; font-family: var(--font); }
      #root { width: 100%; height: 100%; display: flex; align-items: center; }
      .third { display: flex; align-items: stretch; border-radius: var(--radius); overflow: hidden; background: var(--panel); color: var(--ink); animation: in 0.5s var(--ease) both, out 0.3s ease-in both 4.2s; }
      .stripe { width: 10px; background: var(--accent); transform-origin: top; animation: stripe 0.4s var(--ease) 0.15s both; }
      .text { padding: 22px 32px; }
      .name { font-size: 40px; font-weight: 700; letter-spacing: -0.01em; }
      .role { margin-top: 4px; font-size: 26px; color: var(--muted); }
      @keyframes in { from { opacity: 0; transform: translateX(-40px); } to { opacity: 1; transform: none; } }
      @keyframes out { to { opacity: 0; transform: translateX(-24px); } }
      @keyframes stripe { from { transform: scaleY(0); } to { transform: none; } }
    </style>
  </head>
  <body>
    <div id="root" data-composition-id="lower-third" data-no-timeline data-start="0" data-duration="4.5" data-width="768" data-height="160">
      <div class="third clip" data-start="0" data-duration="4.5" data-track-index="0">
        <div class="stripe"></div>
        <div class="text">
          <div class="name">Eden Reich</div>
          <div class="role">Maintainer, Inference Gateway</div>
        </div>
      </div>
    </div>
  </body>
</html>
```

### callout (`x`, `y` at the arrow tip, `width: 0.3`)

The arrow points up-left at the frame position given by `x`/`y`; flip it with `.flip` to point
up-right when the target is near the right edge.

```html
<!doctype html>
<html lang="en">
  <head>
    <meta charset="UTF-8" />
    <style>
      :root { --font: "Inter", -apple-system, "SF Pro Text", "Segoe UI", system-ui, sans-serif; --ink: #fafafa; --muted: #a1a1aa; --accent: #6366f1; --panel: rgba(9, 9, 11, 0.85); --radius: 16px; --ease: cubic-bezier(0.2, 0.8, 0.2, 1); }
      * { margin: 0; padding: 0; box-sizing: border-box; }
      html, body { width: 576px; height: 200px; overflow: hidden; background: transparent; font-family: var(--font); }
      #root { width: 100%; height: 100%; position: relative; }
      .callout { position: absolute; left: 0; top: 0; animation: in 0.45s var(--ease) both, out 0.3s ease-in both 3.2s; }
      .flip { left: auto; right: 0; transform: scaleX(-1); }
      .flip .label { transform: scaleX(-1); }
      .arrow { width: 90px; height: 90px; stroke: var(--accent); fill: none; stroke-width: 8; stroke-linecap: round; stroke-linejoin: round; stroke-dasharray: 200; stroke-dashoffset: 200; animation: draw 0.5s var(--ease) 0.1s both; }
      .label { position: absolute; left: 70px; top: 70px; white-space: nowrap; padding: 16px 24px; border-radius: var(--radius); background: var(--panel); color: var(--ink); font-size: 28px; font-weight: 600; animation: in 0.4s var(--ease) 0.35s both; }
      @keyframes in { from { opacity: 0; transform: translateY(10px); } to { opacity: 1; transform: none; } }
      @keyframes out { to { opacity: 0; } }
      @keyframes draw { to { stroke-dashoffset: 0; } }
    </style>
  </head>
  <body>
    <div id="root" data-composition-id="callout" data-no-timeline data-start="0" data-duration="3.5" data-width="576" data-height="200">
      <div class="callout clip" data-start="0" data-duration="3.5" data-track-index="0">
        <svg class="arrow" viewBox="0 0 100 100"><path d="M80 80 C 60 70, 30 50, 14 16 M14 16 l 4 26 M14 16 l 26 4" /></svg>
        <div class="label">Click Settings here</div>
      </div>
    </div>
  </body>
</html>
```

### step (`x: 0.04, y: 0.06, width: 0.18`)

```html
<!doctype html>
<html lang="en">
  <head>
    <meta charset="UTF-8" />
    <style>
      :root { --font: "Inter", -apple-system, "SF Pro Text", "Segoe UI", system-ui, sans-serif; --ink: #fafafa; --muted: #a1a1aa; --accent: #6366f1; --panel: rgba(9, 9, 11, 0.85); --radius: 16px; --ease: cubic-bezier(0.2, 0.8, 0.2, 1); }
      * { margin: 0; padding: 0; box-sizing: border-box; }
      html, body { width: 346px; height: 96px; overflow: hidden; background: transparent; font-family: var(--font); }
      #root { width: 100%; height: 100%; display: flex; align-items: center; }
      .step { display: flex; align-items: center; gap: 18px; padding: 12px 24px 12px 12px; border-radius: 48px; background: var(--panel); color: var(--ink); animation: in 0.45s var(--ease) both, out 0.3s ease-in both 5.7s; }
      .n { width: 64px; height: 64px; border-radius: 50%; display: grid; place-items: center; background: var(--accent); font-size: 34px; font-weight: 800; animation: pop 0.5s var(--ease) 0.1s both; }
      .t { font-size: 28px; font-weight: 600; }
      @keyframes in { from { opacity: 0; transform: translateY(-16px); } to { opacity: 1; transform: none; } }
      @keyframes out { to { opacity: 0; } }
      @keyframes pop { from { transform: scale(0.4); } 70% { transform: scale(1.12); } to { transform: none; } }
    </style>
  </head>
  <body>
    <div id="root" data-composition-id="step" data-no-timeline data-start="0" data-duration="6" data-width="346" data-height="96">
      <div class="step clip" data-start="0" data-duration="6" data-track-index="0">
        <div class="n">1</div>
        <div class="t">Open Settings</div>
      </div>
    </div>
  </body>
</html>
```

### bar-chart (full frame or `width: 0.5`)

Values live in the `--v` custom property of each bar as a fraction of the tallest; the labels and
numbers are plain text. Three to six bars read well.

```html
<!doctype html>
<html lang="en">
  <head>
    <meta charset="UTF-8" />
    <style>
      :root { --font: "Inter", -apple-system, "SF Pro Text", "Segoe UI", system-ui, sans-serif; --ink: #fafafa; --muted: #a1a1aa; --accent: #6366f1; --panel: rgba(9, 9, 11, 0.85); --radius: 16px; --ease: cubic-bezier(0.2, 0.8, 0.2, 1); }
      * { margin: 0; padding: 0; box-sizing: border-box; }
      html, body { width: 1920px; height: 1080px; overflow: hidden; background: transparent; font-family: var(--font); }
      #root { width: 100%; height: 100%; display: flex; align-items: center; justify-content: center; }
      .chart { width: 1100px; padding: 48px 56px; border-radius: var(--radius); background: var(--panel); color: var(--ink); animation: in 0.5s var(--ease) both, out 0.35s ease-in both 5.65s; }
      h2 { font-size: 40px; font-weight: 700; margin-bottom: 32px; }
      .bars { display: flex; align-items: flex-end; gap: 40px; height: 360px; }
      .bar { flex: 1; display: flex; flex-direction: column; align-items: center; justify-content: flex-end; height: 100%; }
      .fill { width: 100%; height: calc(var(--v) * 100%); border-radius: 8px 8px 0 0; background: var(--accent); transform-origin: bottom; animation: grow 0.7s var(--ease) both; }
      .bar:nth-child(2) .fill { animation-delay: 0.1s; } .bar:nth-child(3) .fill { animation-delay: 0.2s; } .bar:nth-child(4) .fill { animation-delay: 0.3s; } .bar:nth-child(5) .fill { animation-delay: 0.4s; } .bar:nth-child(6) .fill { animation-delay: 0.5s; }
      .val { font-size: 28px; font-weight: 700; margin-bottom: 10px; animation: in 0.4s var(--ease) 0.6s both; }
      .lbl { font-size: 24px; color: var(--muted); margin-top: 14px; }
      @keyframes in { from { opacity: 0; transform: translateY(20px); } to { opacity: 1; transform: none; } }
      @keyframes out { to { opacity: 0; } }
      @keyframes grow { from { transform: scaleY(0); } to { transform: none; } }
    </style>
  </head>
  <body>
    <div id="root" data-composition-id="bar-chart" data-no-timeline data-start="0" data-duration="6" data-width="1920" data-height="1080">
      <div class="chart clip" data-start="0" data-duration="6" data-track-index="0">
        <h2>Requests per second</h2>
        <div class="bars">
          <div class="bar" style="--v: 0.35"><div class="val">1.2k</div><div class="fill"></div><div class="lbl">v1</div></div>
          <div class="bar" style="--v: 0.6"><div class="val">2.1k</div><div class="fill"></div><div class="lbl">v2</div></div>
          <div class="bar" style="--v: 1"><div class="val">3.5k</div><div class="fill"></div><div class="lbl">v3</div></div>
        </div>
      </div>
    </div>
  </body>
</html>
```

## Notes

- Keep `cards/` in the project; it is small and lets the user ask for edits later.
- HyperFrames prints a lot; `--quiet` keeps only warnings. A `media_readiness_timeout` warning means an
  image or font could not load: cards must not reference files outside the working directory.
- Never open the rendered file or play it; the desktop shows it on the timeline.
