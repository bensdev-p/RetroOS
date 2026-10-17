# RetroOS Style Guide

The alternate history: **Phosphor Computer Company** (PCC) was founded in 1981
building terminals, shipped the PCC Model 1 in 1984, and in 1990 released
**RetroOS**: a text-mode environment (**RetroDOS**) that could switch, live, into
a graphical **Desktop**. PCC took the best ideas from both camps of the era —
the command line and drive letters of the PC world, the menu bar, Finder and
startup chime of the Mac world, the Program Manager and chunky windows of
Windows 3.1, the Filer of ProDOS — and made them one product.

Everything we ship should look like it came off the same 1993 floppy set.
**Borrow idioms, never artwork**: no copied icons, fonts, sounds, or logos.

---

## 1. Principles

1. **One product, one hand.** Same palette, same fonts, same outline weight, same voice, in text and GUI mode.
2. **Pixels are pixels.** Integer scaling only. No anti-aliasing in our UI. No gradients except dithers.
3. **Nothing moves that doesn't need to.** No animation, fades, blur, transparency, or shadows other than the 1-px hard drop shadow.
4. **Keyboard first, mouse welcome.** Every action has a key; menus have mnemonics.
5. **Terse, friendly, technical.** Copy reads like a good 1990 manual.
6. **Authentic, not hostile.** Retro quirks are fun only when they don't cost the user (e.g. we never force 8.3 names).

---

## 2. Palette — "Phosphor16"

The only colours our UI uses. Designed for us: CGA-like roles, slightly warmer
and less garish, all pairs used for text pass WCAG AA at our sizes.

| # | Name | Hex | Role |
|---|---|---|---|
| 0 | Black | `#0A0A0F` | outlines, text |
| 1 | Cobalt | `#1F3BB3` | RetroDOS background, active title bar |
| 2 | Fern | `#1E9A3C` | success, OK |
| 3 | Teal | `#1A9FA0` | selection in text mode, links |
| 4 | Brick | `#B2302B` | errors, destructive |
| 5 | Plum | `#8E3A9E` | accent |
| 6 | Ochre | `#B07A1E` | warning, folders |
| 7 | Silver | `#BDBDB8` | window face, menus, default text-mode foreground |
| 8 | Slate | `#5C5C66` | shadows, disabled text |
| 9 | Sky | `#5C7CFA` | highlight/selection (GUI) |
| 10 | Lime | `#6FE06A` | green-phosphor scheme |
| 11 | Aqua | `#5FE3E3` | info |
| 12 | Coral | `#FF6A5C` | alerts |
| 13 | Orchid | `#E27BF0` | accent |
| 14 | Lemon | `#F7E15A` | hotkeys/mnemonics in text mode, amber scheme base |
| 15 | White | `#F5F5F0` | highlights, bright text, paper |

### Colour schemes (selectable: `COLOR` / Control Panel → Display)

| Scheme | Text mode (fg/bg) | GUI |
|---|---|---|
| **Phosphor Classic** (default) | Silver on Cobalt, Lemon hotkeys, Teal selection | Silver faces, Cobalt titles, Sky selection, dithered Cobalt/Black desktop |
| **Paper** | Black on White | Black-and-white System-6 look, 50% gray desktop dither |
| **Amber** | `#FFB000`-tinted ramp (Lemon/Ochre) on Black | Amber monochrome |
| **Green Screen** | Lime on Black | Green monochrome |
| **High Contrast** | White on Black, Lemon selection | Black/White/Lemon only |

---

## 3. Text mode visual language

- **Font**: *Phosphor Mono* 8×16 (primary) and 8×8 (`MODE CON LINES=50`). Our
  own drawing, CP437-complete (box drawing, blocks, shading) + Latin-1 +
  Latin Extended-A + Greek/Cyrillic basics; other glyphs fall back to GNU
  Unifont 8×16/16×16.
- **Grid**: integer-scaled cells; by default the largest scale giving ≥ 80×25
  (1080p → 2× → 120×33). `MODE CON COLS=80 LINES=25` letterboxes with a Cobalt
  overscan border, like a real CRT.
- **Cursor**: blinking underline (default, 2 Hz, 50% duty) at the prompt;
  block in insert-toggle/overtype (`Ins` toggles), exactly like DOS.
- **Mouse**: inverse-video cell block.
- **TUI chrome** (EDIT, Filer, Control Panel…):
  - Top menu bar: Silver background, Black text, Brick mnemonic letter (Lemon and
    White were tried: both too faint on Silver at 2× scale). Open menu title inverted.
  - Dialogs: Silver box, double-line `╔═╗` frame, 1-cell Black drop shadow right/bottom.
  - Buttons: `< OK >` `< Cancel >`, focused button in White.
  - Bottom status/F-key bar: `F1 Help  F3 View  F5 Copy …` keys in Black on Teal.
- **Prompt**: `C:\>` by default; `PROMPT $P$G` semantics. Success is silent;
  errors are one line, no stack traces.

```
╔═══════════════════════════ RetroOS Control Panel ════════════════════════════╗
║  Network   Sound   Display   Keyboard   Date/Time   Users   Startup          ║
╟──────────────────────────────────────────────────────────────────────────────╢
║  Wireless networks:                                                          ║
║   ► HomeNet          ▂▄▆█  WPA3   Connected                                  ║
║     Café-Guest       ▂▄__  Open                                              ║
║                                                                              ║
║            < Join >    < Forget >    < Advanced... >    < Close >            ║
╚══════════════════════════════════════════════════════════════════════════════╝
 F1 Help  Tab Next  Enter Select  Esc Close
```

---

## 4. GUI visual language

Logical pixels below; multiplied by the integer scale.

- **Fonts**
  - *Lantern 12* — system font: proportional, bold-weight strokes (menus, titles, buttons, dialogs).
  - *Lantern 9* — small font: labels under icons, status text.
  - *Phosphor Mono* — DOS Prompt, Jot, code.
- **Window**
  - 4-px resizable frame: Black outer line, Silver body, corner segments marked by notches (Win 3.1).
  - Title bar 18 px: solid **Cobalt** with **White** centred title when active; **Silver** with **Slate** title when inactive.
  - Left: control-menu box (a 14×14 box with a horizontal bar) — click: window menu; double-click: close.
  - Right: minimise `▼` and maximise/restore `▲`/`⬍` buttons, bevelled 1 px White/Slate.
  - Client area outlined 1 px Black. No rounded corners. Hard 1-px Black drop shadow on menus and dialogs only.
- **Bevels**: 1 px White top-left, 1 px Slate bottom-right; pressed state inverts. Never 2-px "3D" bevels (that's 1995).
- **Menus**: Silver, Black text, Sky highlight with White text, mnemonic underlined, `…` for items that open dialogs, `▸` submenus, separators as 1-px Slate etched lines.
- **Desktop**: 8×8 two-colour dither pattern (Cobalt/Black by default), drive icons and Wastebasket on the right edge, minimised windows as icons along the bottom.
- **System Bar**: 20 px, White background, Black text, ◆ Phosphor-menu glyph at left, status items at right, 1-px Black bottom rule.
- **Dialogs**: modal, centred, title bar like windows; icon at left (32×32 Stop/Caution/Note), message in Lantern 12, buttons right-aligned, default button has an extra 1-px Black ring.
- **Selection**: Sky with White text (GUI); Teal with White text (text mode).
- **Move/resize**: XOR outline rectangle while dragging (no live redraw).

---

## 5. Icons and cursors

- **Sizes**: 32×32 (desktop, Program Manager, dialogs) and 16×16 (lists, menus, System Bar).
- **Palette**: Phosphor16 + transparency only.
- **Construction**: 1-px Black outline on every icon; light from top-left; objects shown in a slight ¾ view (documents with folded corner top-right, drives as front-facing boxes); shading by flat colour steps or 50% checkerboard dither — no gradients, no anti-aliasing.
- **Families**: drives (hard disk, floppy-ish USB, SD, network, CD), folders (Ochre), documents (White page + type glyph), apps (object metaphors: a globe-in-window for the browser, a joystick for Arcade, a film reel for Media Player, a pencil-on-pad for Jot, a filing cabinet for File Cabinet, a control-knob panel for Control Panel, a shelf of boxed floppies for Software Library), status (speaker, signal bars, Bluetooth rune, battery), dialogs (Stop = Brick octagon with hand, Caution = Lemon triangle, Note = face-in-speech-bubble).
- **Source format**: plain-text `.icon` files (palette letters per pixel) in `assets/icons/`, compiled to PNG; diff-friendly and reviewable.
- **Retro-fied third-party icons**: auto-generated (downscale → Phosphor16 ordered dither → 1-px outline) when no hand-drawn icon exists.
- **Cursors** (1-px Black outline, White fill): arrow, I-beam, **wristwatch-hourglass** busy cursor (our mashup: an hourglass inside a watch face), crosshair, move, four resize arrows, hand (links), "no". Hotspots documented in the source files.

---

## 6. Sound design

All sounds are **synthesised from code** at build time (`tools/synth`,
patches in `assets/sounds/*.synth`): 22.05 kHz, 16-bit mono, short, slightly
lo-fi (8-bit-ish quantisation + gentle low-pass) — the sound of a 1990 sound
chip, not a sample library.

| Event | Sound | Length |
|---|---|---|
| **Startup chime** | A warm FM-bell C-major-add9 chord (C4·E4·G4·D5), soft attack, long decay. Our signature. | ~1.6 s |
| Shutdown | Same timbre, falling fifth (G4→C4) | ~0.8 s |
| Beep (`BEEP`, `^G`, invalid key) | PC-speaker square wave, 880 Hz | 120 ms |
| Alert (Caution dialog) | Two-note "bloop" sine blip (E5→A5) | 180 ms |
| Error (Stop dialog) | Low square-wave "honk" (A2) with slight detune | 250 ms |
| Notification (updates ready) | Three rising ticks | 200 ms |
| Empty Wastebasket | Filtered noise crumple | 400 ms |
| Disk sounds (optional, **off** by default) | Floppy-style seek clicks during long file copies/app launches | — |

Rules: never more than one UI sound at once; UI sounds follow the master
volume; "Sounds: Off / Essential / All" in Control Panel (Essential = chime +
errors).

---

## 7. Naming and voice

### Names
| Thing | Name |
|---|---|
| Company | Phosphor Computer Company (PCC) |
| Product | RetroOS 1.0 |
| Text mode / shell | RetroDOS, `COMMAND` |
| GUI | RetroOS Desktop |
| Boot screen | Phosphor BIOS |
| Compositor (internal) | Station |
| Launcher | Program Manager |
| File manager (GUI / text) | File Cabinet / Filer |
| Editor (GUI / text) | Jot / EDIT |
| Installer | Software Library |
| Terminal window | DOS Prompt |
| Trash | Wastebasket |
| System menu | ◆ (the "Phosphor menu") |
| Emulator front-end | Arcade |
| Media player | Media Player |
| Task switcher | Task List |
| 16K/4K page kernel switch | Memory Manager |

Commands are written UPPERCASE in docs and help (`DIR`, `RUN FIREFOX`); input
is case-insensitive.

### Voice
- Short sentences. Present tense. Second person. No exclamation marks, no emoji, no "Oops!".
- Name the thing and the fix: `Not enough space on drive D. Free 120 MB and try again.`
- Classic phrasing where it fits and helps: `Bad command or file name`, `File not found`, `Abort, Retry, Fail?`, `It is now safe to turn off your computer.`
- Buttons are verbs: **Install**, **Join**, **Eject**, not "Yes/OK" when a verb is possible.
- Sizes in K/MB/GB (binary); times as `10:42 AM` or 24 h per locale.
- Help text (`/?`) follows MS-DOS layout: one-line description, syntax line, parameter table.

```
C:\>COPY /?
Copies one or more files to another location.

COPY source [destination] [/Y] [/-Y] [/V]

  source       Specifies the file or files to be copied.
  destination  Specifies the directory and/or filename for the new file(s).
  /Y           Overwrites existing files without asking.
  /V           Verifies that new files are written correctly.
```
