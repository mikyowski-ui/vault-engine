use super::{DialogueMusic, MapPreview, Screen};
use crate::{
    AppResult,
    audio::{Channel, MusicRequest, Volumes},
};
use vault_core::{ObjectAction, Perk, WorldEffect};
use vault_render::{IndexedImage, dialogue};

impl MapPreview {
    /// Back to the (possibly newly loaded) local map.
    pub(crate) fn return_to_map(&mut self) {
        // SPEC-013 §3.22: a world or town map opened by map -1 or -2
        // discards the request made while it was open, however it closes.
        self.host.world_map_closed();
        self.screen = Screen::Map;
        self.last_hover = None;
        self.selected = None;
        // A map without a working name (none loaded, SPEC-008 §3.10): its
        // header name titles it instead of the map before it.
        self.path = self.host.world.current_map_name().unwrap_or_else(|| {
            let name = &self.host.world.map.header.name;
            let end = name.iter().position(|b| *b == 0).unwrap_or(name.len());
            String::from_utf8_lossy(&name[..end]).into_owned()
        });
    }
    /// SPEC-008 §3.10 rule 4: a dialogue session's music. Its opening
    /// (`start_gdialog`, after the head window is created) stops the
    /// current track for a talking head, or halves the music volume
    /// without one; its closing (after the windows are destroyed) replays
    /// the last recorded track from its beginning, subject to the music
    /// on/off state, or restores the volume. A session already open
    /// changes nothing, and barter, Review and Ask About keep the session
    /// open. The reference halves the volume a previous no-head session
    /// stored instead of the current one (`[May-improve]`); the safe rule
    /// halves the volume current at the opening and restores it. A volume
    /// of 1 halves to 0, which turns music off by volume (rule 2) until the
    /// restore turns it back on.
    pub(crate) fn dialogue_music_tick(&mut self) {
        let open = self.host.world.dialogue.open;
        match (open, self.dialogue_music) {
            (true, None) => {
                self.dialogue_music = Some(if self.host.world.dialogue.head.is_some() {
                    self.audio.stop_music();
                    DialogueMusic::Stopped
                } else {
                    self.sync_volumes();
                    let volume = self.volumes.music;
                    self.apply_volumes(Volumes {
                        music: volume / 2,
                        ..self.volumes
                    });
                    DialogueMusic::Halved { volume }
                });
            }
            (false, Some(DialogueMusic::Stopped)) => {
                self.dialogue_music = None;
                self.audio.replay_music(self.host.assets.vfs.clone());
            }
            (false, Some(DialogueMusic::Halved { volume })) => {
                self.dialogue_music = None;
                self.apply_volumes(Volumes {
                    music: volume,
                    ..self.volumes
                });
            }
            _ => {}
        }
    }
    pub(super) fn drain_effects(&mut self) {
        self.dialogue_music_tick();
        if !self.host.world.effects.is_empty() {
            self.thaw_view();
        }
        while let Some(effect) = self.host.world.effects.pop_front() {
            match effect {
                WorldEffect::Message(text) => self.notify(text),
                WorldEffect::Elevator(kind) => self.open_elevator(kind),
                WorldEffect::Slideshow | WorldEffect::EndGame => self.endgame = Some(effect),
                WorldEffect::Diagnostic(error) => {
                    tracing::warn!(%error,"world diagnostic");
                    self.status = error.into_bytes();
                }
                WorldEffect::Speech(sound) => self.play_speech(&sound),
                WorldEffect::StopSpeech => {
                    self.audio.stop(Channel::Speech);
                    self.speaking = false;
                }
                WorldEffect::SoundEffect { name, owner } => self.play_effect(&name, owner),
                WorldEffect::WeaponSound { object, name, .. } => {
                    self.play_effect(&name, Some(object))
                }
                WorldEffect::LibrarySound {
                    slot,
                    filename,
                    mode,
                } => self.play_library(slot, &filename, mode),
                WorldEffect::Movie(request) => self.queue_movie(request),
                // SPEC-014 §3.2: the world requests a game fade (`gfade_out`
                // / `gfade_in`, First Aid, Doctor, books) as the standard
                // fade (0) without a forced black start, which blocks.
                WorldEffect::Fade {
                    out,
                    duration,
                    from_black,
                } => {
                    if duration <= 0 && !from_black {
                        self.fades.push_game(out);
                    } else {
                        self.fades.push_library(out, duration, from_black);
                    }
                }
                // The library `fadein` / `fadeout` is timed by its duration
                // and never blocks, `fadeout(0)` included.
                WorldEffect::LibraryFade {
                    out,
                    duration,
                    from_black,
                } => self.fades.push_library(out, duration, from_black),
                WorldEffect::RollHitPoints => self.hud.hp.animate = true,
                WorldEffect::RollArmorClass => self.hud.ac.animate = true,
                WorldEffect::WorldMap => self.open_world_map(),
                WorldEffect::TownMap => self.open_town_map(),
                WorldEffect::MapSaveFailed => self.map_save_failed(),
                // SPEC-008 §3.10: a city track loops.
                WorldEffect::Music(name) => self.request_music(MusicRequest::track(&name, true)),
                // Look and examine already printed their text.
                WorldEffect::DefaultAction {
                    action: ObjectAction::LookAt | ObjectAction::Description,
                    ..
                } => {}
                WorldEffect::DefaultAction { action, target } => {
                    let name = self
                        .host
                        .world
                        .get(target)
                        .map_or(b"object".as_slice(), |object| object.name.as_slice());
                    self.status = [format!("{action:?}: ").as_bytes(), name].concat();
                }
                // SPEC-014 §3.46: a created floating text (a `float_msg` or a
                // taunt) is drawn from the world's floating texts; its
                // effect only asks for the redraw (the view thawed above).
                WorldEffect::Floating { object, .. } | WorldEffect::Taunt { object, .. } => {
                    tracing::trace!(object, "floating text created");
                }
                // The `enhanced` camera's scripted requests (Vanilla never
                // emits them; without the enhanced view they do nothing).
                WorldEffect::Camera(command) => self.camera_command(command),
                other => tracing::debug!(?other, "host presentation request"),
            }
        }
    }
    /// The windows drawn over any screen: the Set Timer and called-shot
    /// windows, a dialog box and the debug overlay.
    pub(super) fn draw_overlays(&self, image: &mut IndexedImage) {
        self.draw_timer(image);
        self.draw_called(image);
        self.draw_dialog(image);
        self.draw_debug(image);
        self.draw_autoplay_hud(image);
    }
    /// The map screen: a conversation's window, or the map view with the
    /// indicator boxes and the interface bar.
    pub(super) fn map_image(&mut self) -> AppResult<IndexedImage> {
        let image = self.map_view_image()?;
        if self.host.world.dialogue.open {
            let font = self
                .font
                .as_ref()
                .ok_or("dialogue requires a loaded FON/AAF font")?;
            // SPEC-014 §3.45: the Ask About window hides the reply window
            // until it shows an answer, and the option window while open.
            let (reply_hidden, options_hidden) = self.talk_hides();
            let dialogue = &mut self.host.world.dialogue;
            let reply = reply_hidden.then(|| std::mem::take(&mut dialogue.resolved_reply));
            let options = options_hidden.then(|| std::mem::take(&mut dialogue.resolved_options));
            // SPEC-019 §3.8: Fallout 2 names its panel art (`di_talkp.frm`
            // for a party member with a control record).
            let panel = self.fallout2_talk_panel();
            let layout = dialogue::compose_with_panel(
                &image,
                &self.host.world.dialogue,
                font,
                &self.palette,
                &self.host.assets,
                self.host.world.perk(Perk::Empathy) > 0,
                self.option_scroll,
                panel.as_deref(),
            );
            let dialogue = &mut self.host.world.dialogue;
            if let Some(reply) = reply {
                dialogue.resolved_reply = reply;
            }
            if let Some(options) = options {
                dialogue.resolved_options = options;
            }
            let layout = layout?;
            if !reply_hidden {
                self.host.world.dialogue.page_offsets = layout.page_offsets;
            }
            self.choices = layout.choices;
            let mut image = layout.image;
            self.draw_talk(&mut image);
            return Ok(image);
        }
        self.choices.clear();
        let mut image = image;
        self.draw_tactical(&mut image, self.view, 1);
        self.draw_autoplay_world(&mut image, self.view, 1);
        Ok(self.compose_world(image))
    }
    /// The barter and loot windows a request waits on. Drawing checks these
    /// too, for a request made after this step, but a session that never
    /// draws still has to open them. A blocking fade holds them, as it holds
    /// the frame. Whether one opened.
    pub(super) fn open_requested_windows(&mut self) -> bool {
        if !matches!(self.screen, Screen::Map) || (self.fade_blocks() && self.fades.freezes()) {
            return false;
        }
        self.check_barter();
        self.check_loot();
        !matches!(self.screen, Screen::Map)
    }
}
