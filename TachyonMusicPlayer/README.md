# Tachyon Music Player

A desktop player for _Tachyon: The Fringe_'s in-flight music: the 451 segments in
`Ttfmusic.sbf`, sequenced by the game's `TTFMUSIC.BIN` script. The decoding, script VM and
player are in `tachyon-core/src/music.rs` (shared with the asset viewer). The format and the
script's logic are described in `decomp/notes/music.md`.

```sh
cargo run --release -p tachyon_music_player -- ../pff             # extracted asset directory
cargo run --release -p tachyon_music_player -- ../GoG/Tachyon.pff  # or the archive itself
```

`Ttfmusic.sbf` is looked for next to the path, in its parent folder and in that folder's
subfolders, as in the viewer. `TTFMUSIC.BIN` comes from the pff.

- **Clips tab:** every segment with its index and length. Clicking one loops it on its own,
  bypassing the script.
- **Themes tab:** every script state, named by the first segment it plays (states marked
  *terminal* are the dispatcher, combat bookkeeping, filler and stinger wait). Clicking one
  restarts the script there, with its variables cleared. The script picks each next segment.
  When the theme ends, the script runs on in the terminal states; after 45 s the theme starts
  again, unless **Free run** is ticked.
- **Hostiles present** sets game variable 0 (the hostile count; **count** sets the number).
  Themes check it between bars. Hostiles appearing switch to the combat states (11, the `f1i1e`
  battle bars). When they leave, the script plays a combat ending: state 9 `FM1Cda` after a big
  fight (more than 6 at the peak), otherwise state 10 `FM1EL`. The change takes effect at the
  next bar.
- **Other game variables:** the rest of the game's slots (dialog cue, job status, shields,
  slot 4, dialog timer, sector, job). The dispatcher (state 1) reads sector and job, so set
  those, then pick state 0 or 1 to hear what the game would choose.
- **Transport:** Play/Pause (Space), Restart, and volume. Up/down step through the list.
- **Now playing:** the mode, the state and segment, progress through the segment, and a history
  of the segments played (with the state each came from).
