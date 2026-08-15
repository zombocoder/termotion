# Termotion

Declarative terminal animation generator for streaming overlays and scene media.

Describe an animation in YAML, render it to WebM, MP4, or a PNG sequence, and drop it
straight into OBS.

```yaml
version: 1
canvas: { width: 1920, height: 1080, fps: 30 }
prompt: { user: zombocoder, host: twitch, path: "~", symbol: "$" }
timeline:
  - type: command
    text: "./brb"
  - type: write_line
    text: "Session suspended."
  - type: write
    text: "Process will resume shortly..."
  - type: pause
    duration: 5s
```

```bash
termotion render brb.yaml --output brb.webm
```

## Status

Milestones M1-M4: validation, deterministic timeline, CPU rendering, PNG sequence
output, and FFmpeg-backed WebM/MP4.

Not yet implemented: `status`, `progress`, `spinner`, transparent WebM/MP4, GIF, live
preview, and CRT/scanline effects.

## Commands

| Command | Purpose |
|---|---|
| `termotion validate <file>` | Check a scenario; reports duration and frame count |
| `termotion inspect <file>` | Print the compiled timeline with timestamps |
| `termotion render <file>` | Render to WebM, MP4, or a PNG sequence |
| `termotion themes list` | List built-in themes |
| `termotion doctor` | Check for FFmpeg and its encoders |

## Requirements

A recent stable Rust toolchain, and FFmpeg with `libvpx-vp9` (WebM) or `libx264`
(MP4). PNG output needs no external tools. Run `termotion doctor` to check.

## Safety

Termotion never executes the commands it renders. `- type: command` with
`text: "shutdown -h now"` draws that text and nothing else.

## License

Apache-2.0.
