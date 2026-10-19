//! SPEC-012 §3.12 game movies in the interactive world: requests from the
//! world (`play_gmovie`, the endgame, the water countdown, the Pip-Boy
//! archive, the startup sequence) queue and play one at a time, modally:
//! the world and its scripts wait. Afterwards the previous screen returns.
use super::{MapPreview, Screen};
use crate::movie::{MoviePlayer, SubtitleStyle};
use vault_core::{
    assets::WorldAssets,
    movie::{self, MoviePolicy, MovieRequest, flags},
};
use vault_render::IndexedImage;

/// A playing movie and the screen it returns to.
pub(crate) struct MovieScreen {
    pub player: MoviePlayer,
    resume: Box<Screen>,
}

impl MapPreview {
    /// The startup sequence: iplogo (fade to black first), then intro.
    pub fn queue_startup_movies(&mut self) {
        // Without the main menu's fade, the preview fades the map in after.
        self.startup_fade = true;
        self.host.play_game_movie(movie::IPLOGO, flags::FADE_BEFORE);
        self.host.play_game_movie(movie::INTRO, 0);
        self.drain_effects();
    }
    pub(crate) fn queue_movie(&mut self, request: MovieRequest) {
        // Headless (a replay): nothing is presented, and the world has
        // already treated playback as ended.
        if self.host.movie_policy == MoviePolicy::CompleteImmediately {
            tracing::debug!(movie = %request.path, "headless: movie completed without playback");
            return;
        }
        self.movies.push_back(request);
        if !matches!(self.screen, Screen::Movie(_)) {
            self.start_next_movie();
        }
    }
    /// Play the next queued movie that opens, from the current screen.
    fn start_next_movie(&mut self) {
        while let Some(request) = self.movies.pop_front() {
            let style = request.subtitles.as_ref().and_then(|_| {
                SubtitleStyle::load(
                    &*self.host.assets.vfs,
                    self.host.assets.font_number(crate::movie::SUBTITLE_FONT),
                )
            });
            match MoviePlayer::load(&*self.host.assets.vfs, request, style) {
                Ok(mut player) => {
                    player.audio = self.audio.clone();
                    let resume = std::mem::replace(&mut self.screen, Screen::Map);
                    self.screen = Screen::Movie(Box::new(MovieScreen {
                        player,
                        resume: Box::new(resume),
                    }));
                    return;
                }
                Err(error) => {
                    tracing::warn!(%error, "movie unavailable");
                    self.status = error.to_string().into_bytes();
                }
            }
        }
        // Every queued movie has ended: release a waiting `playmovie`.
        self.host.movie_finished();
    }
    /// Cleanup: back to the previous screen, then the next queued movie.
    /// Playback ends on a black palette; flag `0x02` fades back to the
    /// game palette (SPEC-012 §3.12). The fade to black that flag `0x01`
    /// requests before playback is hidden behind the movie's own palette.
    fn end_movie(&mut self) {
        let Screen::Movie(screen) = std::mem::replace(&mut self.screen, Screen::Map) else {
            return;
        };
        let MovieScreen { player, resume } = *screen;
        self.fades.set_black();
        if player.request.flags & flags::FADE_AFTER != 0 {
            self.fades.push(false, 0, false);
        } else {
            tracing::debug!(movie = %player.request.path, "movie left the palette black");
        }
        self.screen = *resume;
        self.start_next_movie();
        if self.startup_fade && !matches!(self.screen, Screen::Movie(_)) {
            self.startup_fade = false;
            self.fades.push(false, 0, false);
        }
    }
    pub(crate) fn movie_active(&self) -> bool {
        matches!(self.screen, Screen::Movie(_))
    }
    pub(crate) fn movie_tick(&mut self, now: u64) -> bool {
        let Screen::Movie(screen) = &mut self.screen else {
            return false;
        };
        let changed = screen.player.tick(now);
        if screen.player.finished() {
            self.end_movie();
            return true;
        }
        changed
    }
    /// Input during a movie: a key skips it; a button pressed during the
    /// movie skips it on release.
    pub(crate) fn movie_key(&mut self) -> bool {
        if let Screen::Movie(screen) = &mut self.screen {
            screen.player.key();
            self.end_movie();
            return true;
        }
        false
    }
    pub(crate) fn movie_press(&mut self) -> bool {
        if let Screen::Movie(screen) = &mut self.screen {
            screen.player.press();
            return true;
        }
        false
    }
    pub(crate) fn movie_release(&mut self) -> bool {
        let Screen::Movie(screen) = &mut self.screen else {
            return false;
        };
        screen.player.release();
        if screen.player.finished() {
            self.end_movie();
        }
        true
    }
    pub(crate) fn movie_image(&self) -> Option<IndexedImage> {
        match &self.screen {
            Screen::Movie(screen) => Some(screen.player.image()),
            _ => None,
        }
    }
    /// The movie's device palette replaces the display palette.
    /// A running fade, or black left by a fade or movie, replaces it too.
    pub fn display_override(&self) -> Option<[[u8; 4]; 256]> {
        match &self.screen {
            Screen::Movie(screen) => Some(screen.player.palette()),
            Screen::Help(_) => self.help_display(self.preferences.brightness),
            _ => self
                .fades
                .display(self.game_palette(), self.preferences.brightness),
        }
    }
}
