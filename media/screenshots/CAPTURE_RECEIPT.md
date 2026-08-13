# Skill Deck screenshot capture receipt

- Captured: 2026-08-13 (Europe/Berlin)
- Base commit: `01a91e989fd41fe4812b7f9c5bcd8eeb657310c8`
- Capture-contract diff SHA-256: `BEBC2FB54A765AF9B168375602EB8A193C7A7AA6DE1E3C38D909280AAE714B09`
- Runtime: fresh release build with `tauri/custom-protocol`, product version `0.3.0`
- Runtime SHA-256: `9011D23C5CE452C61960416AD4F5FA98D9CFC6B972536005FAC74A8893400389`
- Capture surface: native packaged WebView2 at `http://tauri.localhost/`, CSS viewport `460 x 640`, device scale factor `2`
- Capture contract: `SKILL_DECK_CAPTURE_ROOT` pointed to a task-owned regular directory with sentinel `.skill-deck-capture-root` containing exactly `skill-deck-capture-v1\n`.
- Synthetic fixture: exactly four regular `.agents/skills/*/SKILL.md` files and zero reparse points.
- Scan proof: exactly 4 skills, 0 scan errors, and all four reported paths contained by the declared capture root.
- Search query: `Synthetic demo artifact`
- Unlock proof: pass. The removed `license_get_status` command was rejected, `harness_builtin_status` returned `Jan-nano 4B`, provider discovery succeeded with one managed provider, Harness Settings and the local model were visible, and no activation, purchase, license-key, perpetual-license, or upgrade copy was present.
- User configuration: Finder state was restored after capture. The configuration timestamp remained `2026-08-06T05:14:22.1666975+02:00` and SHA-256 remained `156FA11326B54920F44FED8C78CBECE9E5B415A8267431123A5D5D189841F56D`.

## Images

| File | Pixels | Bytes | SHA-256 |
| --- | ---: | ---: | --- |
| `skill-deck-searchable-catalog.png` | `920 x 1280` | 131687 | `183420382F4F5E8FB5D43CCE8EE5A4C4A722268B2DF8C4B59537D0E859188380` |
| `skill-deck-agent-settings-unlocked.png` | `920 x 1280` | 130792 | `F480C1F391338F232225DF4D92DDF8CC77C8717E9AAA48992F999533086F0583` |

## Privacy note

The capture contract replaced all normal discovery roots before registry construction. The visible catalogue contains only the four synthetic artifacts, aggregate counts derive only from those four files, and no real HOME artifact, username, private absolute path, key, token, or internal evidence label is present.

## Commands

```powershell
cd src-tauri
cargo build --release --bin skill-deck --features tauri/custom-protocol
Start-Process -FilePath 'target\release\skill-deck.exe' -WorkingDirectory '<capture-root>' -WindowStyle Hidden -Environment $captureEnvironment
node '<temporary-capture-root>\capture.mjs'
Get-FileHash -Algorithm SHA256 -LiteralPath 'media\screenshots\*.png'
```
