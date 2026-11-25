# Owner brief: a camera follow setting in `vault-platform`

Basis: `vault-engine` at revision `4d5183a`. Modules: `vault_core::world`
(`WorldState::set_view_center`, `WorldState::move_object`, in
its spatial module), `vault_core::ui::cursor`
(`inside_border`, `distance_refused`, `BORDER_COLUMNS`, `BORDER_ROWS`),
`vault_core::camera` (`CameraCommand`, `CameraRegion`), `vault_platform::map_preview::camera`
(`Camera`, `Camera::tick`, `Camera::resume_follow`, `Camera::drag`,
`Camera::zoom_to`, in its camera module),
`vault_platform::map_preview::pointer` (`center_on_player`),
`vault_platform::enhanced` (`EnhancedSettings`), `vault_platform::config`
(`EnhancedProfile`), `vault_platform::input` (`InputCommand`,
`EnhancedRecord`), `vault_formats::mod_manifest` (the `[enhanced]` table) and
`vault_formats::savegame` (the `viewport` field). Specifications: SPEC-005
§2.5 "Objects, Geometry Queries, Visibility, and Inventories" (word `0x80B6`
`move_to`: "Player movement recenters") and §2.9 "Combat, Time, Lighting,
and Map Transitions" (words `0x80A8` `set_map_start`, `0x80A9`
`override_map_start`, `0x80F8` `tile_is_visible`); SPEC-008 §2.5 "Ordered
`SAVE.DAT` Payloads" (payload 5 ends with the view-centre hex); SPEC-013 §3.21 "Elevators"
step 6 (an elevation change recentres); SPEC-014 §3.19 "Interface Bar:
Window, Buttons and Hotkeys" (Home: "Center on the player"), §3.29 "Edge
Scrolling" (step size, scroll limiting, map border, the Home walk, arrow
keys); SPEC-017 §5.2 "`metarule3`" rule 108 `METARULE3_TILE_SET_CENTER`; SPEC-020 §2.2
"Unified world and viewport stepping" and §2.6 "Hotkey toasts"; SPEC-021
§3.1 "What the Hash Covers, and Where Each Value Comes From" and §5.10 "The Scenario Logs (Revision 4)".

Tags: `[Must-match]` must be exactly as stated, and leaves every existing
result unchanged; `[May-improve]` an implementation freedom that changes no
stated result; `[Safe rule]` what the engine does with a value or state it
cannot honour.

## 1. Purpose

Players and viewers of total-conversion and large content mods report that
a view that tracks the character continuously is disorienting, and that a
view that recentres on every hex step makes long walks hard to watch. Mod
authors and documentation tools also need fixed, stationary shots of a map
that a replay reproduces exactly. This brief asks for one `camera_follow`
setting that selects how the view follows the player, an inner-margin
safety net so the player is never lost, a documented recentre key, and a
replay record and frame option for stationary shots.

What the engine does today (`4d5183a`):

1. **Vanilla preset.** `WorldState::move_object` calls
   `set_view_center(hex)` on every placement of the player, so a walk
   recentres the view once per hex step. Map entry, elevation changes,
   `move_to`, `override_map_start`, metarule3 108 and floating-message style
   -2 recentre too. The view centre is the hex `WorldState::camera_hex`.
2. **Border.** `set_view_center` refuses a centre outside the fixed map
   border (`inside_border`: mirrored columns 44-154, that is columns 45-155,
   and rows 45-154, under the §3.29 safe rule) and leaves the view where it was (SPEC-014 §3.29).
3. **Manual movement.** Edge-pixel scrolling and the arrow keys move the
   view 32 by 24 pixels per step through the 33-millisecond gate, under scroll
   limiting (`distance_refused`) and the scroll-blocker test. Home walks the
   view hex by hex toward the player under the same tests, stopping at the
   first refused step (`center_on_player`); it is not an instant jump.
4. **Enhanced preset, unlocked viewport.** `Camera::new` starts with
   `follow: true`. While following, the target is the player's drawn
   position (hex centre plus displacement) every tick, eased with a
   120-millisecond time constant when `smooth_camera` is on, and snapped
   when it is off or when the player jumps more than 640 pixels. Following
   stops on a middle-button or Space drag, an edge or arrow pan, a wheel or
   key zoom anchored away from the centre, and a scripted `op_cam_pan_to`;
   Home (`resume_follow`) and `op_cam_reset` resume it. The world's
   `camera_hex` is kept at the hex nearest the camera centre.
5. **Settings.** `EnhancedSettings` has no follow field. The
   `vault-engine.toml` enhanced keys are `preset`, `viewport_mode`,
   `default_zoom`, `smooth_camera`, `roof_peek`, `ui_scale`, `game_speed` and
   `tactical_overlay`; a layer's `vault-mod.toml` `[enhanced]` table carries
   the camera keys too.
6. **Replays.** `InputCommand` has no camera record; the `enhanced` record
   carries `viewport_mode`, `default_zoom`, `smooth_camera`, `roof_peek`,
   `ui_scale` and the optional `navigation_only` and `tactical_overlay`.
   `--replay-frame` writes only the final frame. The replay digest covers
   the presented frame, so the camera reaches the digest although it is
   outside `state_hash` (SPEC-021 §3.1).
7. **Specs.** Per-step recentring under Vanilla is specified behaviour:
   SPEC-005 `0x80B6`, SPEC-013 §3.21 and SPEC-014 §3.29 rely on it, and the
   SPEC-021 §5.10 scenario `scenario-explore-talk` records "the view follows
   it" for steps 11-133.

## 2. Proposed settings and API

In `vault-engine.toml` (top level and any profile table), the `[enhanced]`
table of `vault-mod.toml`, the launcher and the command line:

```toml
camera_follow = "off"        # "off" | "step" | "smooth"
camera_recenter_margin = 96  # view pixels at zoom 1; 0 turns the safety net off
```

In `vault_platform::enhanced`:

```rust
/// How the view follows the player.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CameraFollow {
    /// The view moves only by edge or arrow scroll, drag, zoom, scripted
    /// pans, map entry, elevation change, the recentre key and the margin rule.
    Off,
    /// Recentre on the player's hex at every placement (today's Vanilla rule).
    Step,
    /// Track the player's drawn position every tick (today's enhanced rule).
    Smooth,
}

pub struct EnhancedSettings {
    // existing fields ...
    pub camera_follow: CameraFollow,
    /// View pixels at zoom 1 between the player's drawn hex centre and each
    /// view edge before the view recentres; 0 is off.
    pub camera_recenter_margin: u16,
}
```

In `vault_platform::input`, a new record and an `enhanced` record field:

```rust
pub enum InputCommand {
    // existing variants ...
    /// Place the view (tools, and the F9 Camera page): applied at its place
    /// in the log, before the next tick.
    Camera { hex: i32, offset: [i32; 2], zoom: f32, follow: CameraFollow },
}

pub struct EnhancedRecord {
    // existing fields ...
    /// Written only when it differs from the legacy behaviour, so older
    /// logs read as the legacy value.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub camera_follow: Option<CameraFollow>,
}
```

Command line: `--camera-follow off|step|smooth`, and for replays
`--replay-frames DIR` with `--replay-frame-every N` (default 1).

## 3. Behaviour

### 3.1 Modes

1. `step` is exactly today's Vanilla rule: every placement of the player
   calls `set_view_center` on the player's hex, with the border test and no
   scroll limiting (SPEC-005 `0x80B6`). `[Must-match]`
2. `smooth` is exactly today's enhanced `Camera` follow, including the
   120-millisecond easing, the snap rule, and every event that stops or
   resumes following (section 1, item 4). `[Must-match]`
3. `off`: a placement of the player does not move the view. The view still
   moves for edge and arrow scrolling, a drag, a zoom, a scripted camera
   word, `move_to` and `override_map_start` on the player, metarule3 108,
   floating-message style -2, map entry, an elevation change, the recentre
   key, a load, and the margin rule (section 3.2). `[Safe rule]`
4. Under `off` the enhanced `Camera` starts with `follow` false and never
   sets it from a player move; the recentre key centres once (section 3.3)
   and leaves `follow` false. `[Safe rule]`
5. Every centre a mode produces passes `set_view_center`'s border test as
   today; a refused centre leaves the view where it was (SPEC-014 §3.29).
   `[Must-match]`
6. Scroll limiting and the scroll-blocker test apply to manual scrolling
   under every mode exactly as today. `[Must-match]`

### 3.2 Margin rule (never lose the player)

1. With `camera_recenter_margin` M above 0 and mode `off`, after each
   placement of the player, the engine computes the player's drawn hex
   centre in view pixels at zoom 1 (zoom-scaled under the unlocked view).
   `[Safe rule]`
2. When that point lies less than M pixels from any view edge, or outside
   the view, the view recentres on the player's hex (Vanilla, or the
   fixed enhanced view) or snaps or eases to the player's drawn position
   (unlocked view, by `smooth_camera`). `[Safe rule]`
3. The test uses the view's own size: 640 by 380 for the Vanilla map
   window. A margin larger than half the view's width or height is clamped
   to that half minus one pixel. `[Safe rule]`
4. The rule runs only at player placements, never per tick, so a stationary
   player never moves the view. `[Safe rule]`
5. M is ignored under `step` and `smooth`. `[Safe rule]`

### 3.3 Recentre key and hints

1. Home stays the recentre key under both presets. Under Vanilla and `step`
   it keeps the SPEC-014 §3.29 hex-by-hex walk. `[Must-match]`
2. Under `off` the key centres the view on the player once, by the same
   walk (Vanilla, fixed view) or one snap or ease (unlocked view).
   `[Safe rule]`
3. Under `smooth` the key resumes following as today. `[Must-match]`
4. The F1 help text and `docs/REFERENCE.md` name the key. A SPEC-020 §2.6
   hint row `Home: Center view` joins the hotkey toasts after row 6, shown
   when `camera_follow` is `off`. `[Safe rule]` (the toast texts are
   `[Must-match]` in SPEC-020, so this is a revision of that table)
5. A shell key toggles the mode in play, `off` then `smooth` then `off`
   under the enhanced preset (`step` is reached only from settings), and
   the HUD names the new mode, as Alt+F does for game speed. Proposed key:
   Alt+C, unused at `4d5183a`. It is logged as an `enhanced` record, not as
   a key, as Alt+V is. `[Safe rule]`
6. The launcher's enhanced settings gain a `Camera follow` row that steps
   through `Off`, `Step` and `Smooth`, and a `Recentre margin` row.
   `[Safe rule]`

### 3.4 Defaults

1. Enhanced preset: `camera_follow = "off"`, `camera_recenter_margin = 96`.
   `[Safe rule]` (owner decision, question 1)
2. Vanilla preset: `step`, fixed, not settable (recommended). `[Must-match]`
   (owner decision, question 2)
3. The built-in profiles: `VanillaPurist` keeps `step`; `WidescreenClassic`
   and `ModernTactical` take `off`. `[Safe rule]`

### 3.5 Replay record and frames

1. A `camera` record places the view at hex `hex` plus `offset`
   default-grid pixels, at `zoom`, with following set by `follow`, before
   the next tick is applied. `[Safe rule]`
2. Under Vanilla and the fixed view, `offset` must be `[0, 0]` and `zoom`
   1.0; any other value refuses the record before the first step, as a
   usage error. `[Safe rule]`
3. A `hex` outside 0-39999 or outside the border, a zoom outside the
   view's floor to 3.0 range, or a non-finite zoom is a usage error before
   the first step. `[Safe rule]`
4. The desktop writes a `camera` record only from the F9 Camera page's
   "place camera" action; ordinary drags and keys stay logged as the
   pointer and key records they are. `[Safe rule]`
5. `--replay-frames DIR` writes the presented frame after every N-th
   `tick` record (N from `--replay-frame-every`, at least 1) as
   `DIR/frame-<tick index, 6 digits>.ppm`, in the format `--replay-frame`
   writes. The digest is unchanged by the option. `[Safe rule]`
6. A log whose `enhanced` record has no `camera_follow` replays with the
   legacy behaviour: `smooth` under the enhanced unlocked view, `step`
   elsewhere. So every existing log, golden and gallery image keeps its
   bytes. `[Must-match]`

## 4. Validation and errors

1. An unknown `camera_follow` value in `vault-engine.toml` is the same
   diagnostic as an unknown `roof_peek` value, and the default applies.
   `[Safe rule]`
2. In `vault-mod.toml` an unknown value, or a margin outside 0-1024, is a
   manifest error that stops the launch, as an unknown `[enhanced]` value
   does today. `[Safe rule]`
3. `camera_follow` other than `step` under the Vanilla preset is refused
   with a diagnostic and `step` applies (recommended default of question 2).
   `[Safe rule]`
4. A malformed `camera` record is a usage error naming its `command_id`.
   `[Safe rule]`

## 5. Determinism property

1. The camera stays presentation state. `camera_follow`, the margin, the
   toggle key and the `camera` record change no value covered by
   `state_hash` or `combat_hash`, no checkpoint row, and no random draw.
   `[Must-match]` (SPEC-021 §3.1 already lists the camera as not covered)
2. `camera_hex` keeps feeding what reads it today: the save's `viewport`
   field, `tile_is_visible` (SPEC-005 `0x80F8`) and the scroll tests. Under
   `off` those read the view the player actually has. A script that reads
   `tile_is_visible` can therefore return a different value under `off`
   than under `step`; this is the one world-visible effect, and it is the
   reason Vanilla keeps `step` (question 2). `[Safe rule]`
3. Every camera update is a function of the logged shell clock and logged
   input, so two runs of one log give identical frames and digests.
   `[Must-match]`
4. The save layout does not change: the `viewport` field still holds the
   view-centre hex (SPEC-008 §2.5, payload 5). `[Must-match]`

## 6. Edge cases

1. A walk that crosses the margin on its last step: the recentre happens on
   that placement.
2. An elevation change and a map change under `off`: the view centres on
   the player as today (SPEC-013 §3.21, SPEC-014 §3.29).
3. The player outside the border (an edge hex): the recentre is refused and
   the view stays; the margin rule does not retry each tick.
4. A scripted `op_cam_pan_to` during `off`: it runs as today; `op_cam_reset`
   under `off` returns to the default zoom and centres once, with `follow`
   left false.
5. Combat under `off`: NPC turns do not move the view; the margin rule
   still runs for the player's own moves.
6. A conversation, barter, or any window over the map: no placement
   happens, so nothing changes.
7. Zoom 3.0 with a margin of 96: the margin is measured in view pixels at
   zoom 1, so it covers 32 world pixels.
8. `--replay-frame-every 0`: refused as a usage error.

## 7. Synthetic test vectors

All on a synthetic 200-by-200 empty map with no scroll blockers, view 640
by 380, zoom 1, `smooth_camera = false`. Hex steps follow the SPEC-003 §3.4
neighbour table: southeast (orientation 2) adds 200 and moves the hex
centre by (16, 12).

Vector A, a five-hex walk southeast from hex 20100 (column 100, row 100):
player hexes 20300, 20500, 20700, 20900, 21100. Expected view centres after
each placement:

| Step | Player hex | `step` | `smooth` (snap) | `off`, margin 96 | `off`, margin 0 |
| ---: | ---: | ---: | ---: | ---: | ---: |
| 0 | 20100 | 20100 | 20100 | 20100 | 20100 |
| 1 | 20300 | 20300 | 20300 | 20100 | 20100 |
| 2 | 20500 | 20500 | 20500 | 20100 | 20100 |
| 3 | 20700 | 20700 | 20700 | 20100 | 20100 |
| 4 | 20900 | 20900 | 20900 | 20100 | 20100 |
| 5 | 21100 | 21100 | 21100 | 20100 | 20100 |

Under `off` after step 5 the player's hex centre is at view point
(320 + 80, 190 + 60) = (400, 250).

Vector B, the margin rule: the same walk continued to ten steps. The margin
box for M = 96 is x 96-544, y 96-284. After step k the player is at
(320 + 16k, 190 + 12k). Step 8 gives y = 286, past 284, so the view
recentres on 21700 (row 108). Expected centres for steps 0-10:
20100 × 8 (steps 0-7), then 21700, 21700, 21700.

Vector C, the record: a log of `surface`, `enhanced` with
`camera_follow = "off"`, `camera {hex: 21100, offset: [0, 0], zoom: 1.0,
follow: "off"}`, ten `tick` records and `quit`, run with
`--replay-frames out --replay-frame-every 5`, writes `out/frame-000004.ppm`
and `out/frame-000009.ppm`, both centred on hex 21100; the digest equals the
same log's digest without the option.

Vector D (refused): `camera_follow = "follow"`; a `camera` record with hex
40000; under Vanilla a `camera` record with zoom 2.0; `--replay-frame-every 0`.

Vector E (legacy): an existing enhanced log with no `camera_follow` field
gives the same digest before and after the change.

## 8. Open questions for the owner

1. **Enhanced default.** Should the enhanced preset default to `off`?
   Recommended: yes, with margin 96; `smooth` stays one key away.
2. **Vanilla default (flagged: current specs tag per-step recentring as
   intended behaviour).** Should the Vanilla preset keep `step` as its fixed
   `[Must-match]` behaviour, or may a player choose `off` there too?
   Recommended: Vanilla stays as specified (`step`, not settable), so SPEC-005,
   SPEC-013, SPEC-014 and the SPEC-021 scenario goldens are untouched; the
   choice lives in the enhanced preset, including its fixed 640x480 view.
3. **`tile_is_visible` under `off`.** Should it keep reading `camera_hex`,
   or read a "virtual follow" hex equal to the player's so scripts behave as
   under `step`? Recommended: read the player's hex while `off` is active,
   so script results never depend on the setting.
4. **Margin units.** View pixels at zoom 1, or a fraction of the view?
   Recommended: pixels at zoom 1 (96), clamped as section 3.2 says.
5. **Recentre during combat.** Should the margin rule also run for NPC moves
   in combat so the acting critter is shown? Recommended: no; the player's
   moves only.
6. **Toggle key.** Is Alt+C acceptable? Recommended: yes; it is free at
   `4d5183a` and follows the Alt+V / Alt+F shell-key pattern.
7. **Hint row.** Is a seventh SPEC-020 hint acceptable, given that the
   toast texts are `[Must-match]`? Recommended: yes, shown only under `off`,
   recorded as a SPEC-020 revision.
8. **Frame file names.** Six-digit tick index, or the shell clock in
   milliseconds? Recommended: tick index, so names do not depend on frame
   timing.

## 9. Out of scope

- New camera script words; the existing `0x8210`-`0x8213` words are
  unchanged.
- Cinematic paths, keyframed camera tracks and per-map camera presets.
- Changing scroll limiting, the map border or the scroll-blocker rule.
- Recording drags and zooms as camera records (they stay input records).

## 10. Owner decisions and what was built (2026-11-24)

1. Question 1: yes. The Enhanced preset defaults to `off` with margin 96.
2. Question 2: Vanilla keeps `step`, not settable. A `camera_follow` other
   than `step` under Vanilla is refused with a diagnostic. The Vanilla touch
   view keeps its smooth follow.
3. Question 3: under `off`, `tile_is_visible`, `obj_on_screen` and the
   idle-animation candidates read the player's hex, so script results and
   random draws do not depend on the setting. Section 5.1 holds only
   because of this: those three read the view centre.
4. Questions 4 and 5: as recommended (pixels at zoom 1; the player's moves
   only).
5. Question 6: Alt+C, as a shell key under the Enhanced preset only. Under
   Vanilla it still reaches the game as `c`. It is logged as an `enhanced`
   record.
6. Question 7: yes, as the seventh row; `Alt+L` becomes the eighth
   (SPEC-020 Revision 6).
7. Deferred to a later wave: the `camera` replay record, the F9 Camera
   page's place action, `--replay-frames` and `--replay-frame-every`
   (section 3.5 items 1-5, vector C, and the record parts of vector D).

Corrections to the text above:

- Section 1 item 7: no approved spec says the view recentres on every hex
  step of a walk. The specs name `move_to`, `override_map_start`,
  `set_map_start`, map entry, an elevation change, floating-message style
  -2, metarule3 108 and Home.
- Section 3.3.4: F1 is an art screen with no text to edit.
- Section 4.1: an unknown `roof_peek` value fails the file, and
  `camera_follow` does the same; no default applies.
- `EnhancedSettings::default()` keeps the legacy `smooth`, which is how a
  log without the field reads; a configuration that names no value gets
  `off`.
