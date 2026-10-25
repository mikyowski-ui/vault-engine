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
        self.countdown_movie = false;
    }
    /// §3.13 `play_gmovie`: the table flags (plus `0x02` for indexes 3 and 4
    /// without a loaded map). An index outside 0..13 is a failed play
    /// (safe rule); a failed play is a diagnostic only.
    pub(crate) fn play_gmovie(&mut self, index: i32) {
        if self.world.fallout2.is_some() {
            self.fallout2_gmovie(index);
            return;
        }
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
    /// SPEC-017 §3.1 `play_gmovie` under Fallout 2: SPEC-018 §6.4's
    /// 17-entry table with SPEC-019 §3.21's flags `0x0B`, and its own
    /// played record. An index outside 0-16, or a movie not found, is a
    /// failed play: a diagnostic only.
    fn fallout2_gmovie(&mut self, index: i32) -> bool {
        let Some(file) = usize::try_from(index)
            .ok()
            .and_then(|index| crate::fallout2::MOVIES.get(index))
        else {
            self.world
                .diagnostic(format!("play_gmovie: movie {index} is outside 0-16"));
            return false;
        };
        let path = format!("art/cuts/{file}");
        if !self.assets.available(path.as_bytes()) {
            self.world.diagnostic(format!("{path}: movie not found"));
            return false;
        }
        let subtitles = self
            .subtitles_enabled()
            .then(|| movie::subtitle_path(self.assets.language(), &path))
            .filter(|subtitles| self.assets.available(subtitles.as_bytes()));
        let fades =
            Some(movie::fade_path(&path)).filter(|fades| self.assets.available(fades.as_bytes()));
        if let Some(state) = self.world.fallout2.as_deref_mut() {
            state.movies[index as usize] = 1;
        }
        self.world.emit(WorldEffect::Movie(MovieRequest {
            index: None,
            flags: 0x0b,
            path,
            subtitles,
            fades,
        }));
        true
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
        // SPEC-019 §3.10: Fallout 2's archives scan indexes 2-15 of its own
        // played record.
        if let Some(state) = self.world.fallout2.as_deref() {
            return match crate::ui::fallout2::archive_selection(&state.movies, selection) {
                Some(index) => self.fallout2_gmovie(index as i32),
                None => false,
            };
        }
        match movie::archive_selection(&self.world.persistence.movies, selection) {
            Some(index) => self.play_game_movie(index, movie::ARCHIVE_FLAGS),
            None => false,
        }
    }
    /// §3.12 the midnight water countdown, steps 3-4 (the handler has
    /// already returned while a countdown movie plays, step 1, and unjammed
    /// the locks, step 2). The remaining water is campaign global 10 and the
    /// water-chip quest global 101. Unless the quest is 2, a positive value
    /// decrements; then, outside dialogue, the new value chooses at most one
    /// movie: boil1 at 100 or less with its water-movie bit clear (setting
    /// it), else boil2 at 50 or less with its bit clear (setting it), else
    /// boil3 at 0. A chosen movie plays only if not yet played (boil1 and
    /// boil2 with flags `0x07`, boil3 with `0x05` and then a quit request).
    /// `[May-improve]` safe rule: a value already 0 at a midnight outside
    /// dialogue, with the quest not 2, chooses boil3, so a countdown that
    /// reached 0 during a dialogue still ends the game.
    pub(crate) fn water_countdown(&mut self) {
        let Some(gvars) = self.world.gvars.as_mut() else {
            return;
        };
        if gvars.get(movie::WATER_CHIP_GLOBAL) == Some(&2) {
            return;
        }
        let Some(water) = gvars.get_mut(movie::WATER_GLOBAL) else {
            self.world.diagnostic(format!(
                "water global {} is outside the campaign",
                movie::WATER_GLOBAL
            ));
            return;
        };
        let counted = *water > 0;
        if counted {
            *water -= 1;
        }
        let water = *water;
        if self.world.dialogue.open || !(counted || water == 0) {
            return;
        }
        let chosen = &mut self.world.persistence.campaign_status;
        let (index, flags) = if !counted {
            (movie::BOIL3, 0x05)
        } else if water <= 100 && *chosen & movie::BOIL1_CHOSEN == 0 {
            *chosen |= movie::BOIL1_CHOSEN;
            (movie::BOIL1, 0x07)
        } else if water <= 50 && *chosen & movie::BOIL2_CHOSEN == 0 {
            *chosen |= movie::BOIL2_CHOSEN;
            (movie::BOIL2, 0x07)
        } else if water == 0 {
            (movie::BOIL3, 0x05)
        } else {
            return;
        };
        if self.world.persistence.movies[index] == 0 && self.play_game_movie(index, flags) {
            // Headless, playback ends as it starts (`MoviePolicy`).
            self.countdown_movie = self.movie_policy == movie::MoviePolicy::WaitUntilFinished;
            if index == movie::BOIL3 {
                self.world.emit(WorldEffect::EndGame);
            }
        }
    }
}
