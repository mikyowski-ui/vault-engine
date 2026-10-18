//! SPEC-012 §3.12-3.13 game movie requests: lookup, subtitle and fade
//! files, the played-movie record, and the engine callers (endgame, Pip-Boy
//! archive, midnight water countdown). Presentation is the host's: a found
//! movie becomes a `WorldEffect::Movie` request.
use crate::{
    WorldEffect, WorldHost,
    assets::WorldAssets,
    movie::{self, MovieRequest},
};

/// SPEC-008 preference word 9: subtitles.
const SUBTITLE_PREFERENCE: usize = 9;

impl<A: WorldAssets> WorldHost<A> {
    /// The subtitles preference (SPEC-008 preference word 9).
    pub fn subtitles_enabled(&self) -> bool {
        self.world.persistence.preferences[SUBTITLE_PREFERENCE] != 0
    }
    pub fn set_subtitles(&mut self, enabled: bool) {
        self.world.persistence.preferences[SUBTITLE_PREFERENCE] = u32::from(enabled);
    }
    /// §3.12 playing game movie `index` with `flags`. A movie the lookup
    /// does not find fails: nothing is shown and its played byte is
    /// unchanged (always the case for index 13). Otherwise subtitles are
    /// chosen (always for the boil movies, else by preference, and only if
    /// the `.SVE` exists), the movie is marked played (playing it to the
    /// end and skipping it both mark it), and the request is emitted.
    pub fn play_game_movie(&mut self, index: usize, flags: u32) -> bool {
        let Some(path) = movie::movie_path(index) else {
            self.world
                .diagnostic(format!("game movie {index} is outside the table"));
            return false;
        };
        if !self.assets.available(path.as_bytes()) {
            self.world.diagnostic(format!("{path}: movie not found"));
            return false;
        }
        let subtitles = (movie::subtitles_forced(index) || self.subtitles_enabled())
            .then(|| movie::subtitle_path(self.assets.language(), &path))
            .filter(|subtitles| self.assets.available(subtitles.as_bytes()));
        let fades =
            Some(movie::fade_path(&path)).filter(|fades| self.assets.available(fades.as_bytes()));
        self.world.persistence.movies[index] = 1;
        self.world.emit(WorldEffect::Movie(MovieRequest {
            index: Some(index),
            flags,
            path,
            subtitles,
            fades,
        }));
        true
    }
    /// The presentation layer finished every queued movie: a library movie
    /// started by `playmovie` has ended, which releases its waiting program
    /// (SPEC-005 §3.2 safe rule).
    pub fn movie_finished(&mut self) {
        self.library.movie_playing = false;
    }
    /// §3.13 `play_gmovie`: the table flags (plus `0x02` for indexes 3 and 4
    /// without a loaded map). An index outside 0..13 is a failed play
    /// (safe rule); a failed play is a diagnostic only.
    pub(crate) fn play_gmovie(&mut self, index: i32) {
        let map_loaded = self.world.map.header.name[0] != 0;
        match movie::play_gmovie_flags(index, map_loaded) {
            Some(flags) => {
                self.play_game_movie(index as usize, flags);
            }
            None => self
                .world
                .diagnostic(format!("play_gmovie: movie {index} is outside 0-13")),
        }
    }
    /// The endgame movie: walkm or walkw by the player's gender, flags 0.
    pub fn play_endgame_movie(&mut self) -> bool {
        let player = self.world.player;
        let female = self
            .world
            .get(player)
            .and_then(|object| object.character.as_ref())
            .is_some_and(|sheet| sheet.stat(34, self.world.context(player)) == 1);
        self.play_game_movie(if female { movie::WALKW } else { movie::WALKM }, 0)
    }
    /// The Pip-Boy video archive: selection `a` through §3.12's scan, with
    /// flags `0x0B`. No qualifying movie plays nothing (safe rule).
    pub fn play_archive_movie(&mut self, selection: i32) -> bool {
        match movie::archive_selection(&self.world.persistence.movies, selection) {
            Some(index) => self.play_game_movie(index, movie::ARCHIVE_FLAGS),
            None => false,
        }
    }
    /// The midnight water countdown, with the remaining water in the
    /// host-supplied global (`WorldRules::water_global`; no approved
    /// specification names it, so without one nothing happens). While the
    /// water-chip quest is unfinished the water decrements first; then,
    /// outside dialogue, the most severe threshold reached plays its movie
    /// if not yet played: boil3 at 0 (flags `0x05`, then a quit request),
    /// boil2 at 50 or less and boil1 at 100 or less (`0x07`).
    pub(crate) fn water_countdown(&mut self) {
        let Some(global) = self.world.rules.water_global else {
            return;
        };
        let Some(gvars) = self.world.gvars.as_mut() else {
            return;
        };
        if gvars
            .get(movie::WATER_CHIP_GLOBAL)
            .is_none_or(|quest| *quest > 1)
        {
            return;
        }
        let Some(water) = gvars.get_mut(global) else {
            self.world
                .diagnostic(format!("water global {global} is outside the campaign"));
            return;
        };
        *water = water.saturating_sub(1);
        let water = *water;
        if self.world.dialogue.open {
            return;
        }
        let (index, flags) = match water {
            ..=0 => (movie::BOIL3, 0x05),
            1..=50 => (movie::BOIL2, 0x07),
            51..=100 => (movie::BOIL1, 0x07),
            _ => return,
        };
        if self.world.persistence.movies[index] == 0
            && self.play_game_movie(index, flags)
            && index == movie::BOIL3
        {
            self.world.emit(WorldEffect::EndGame);
        }
    }
}
