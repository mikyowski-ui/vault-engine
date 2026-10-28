//! Unified SessionRuntime facade (SPEC-020 §2.3, Wave 2).
//!
//! Every session the desktop runs goes through one [`SessionRuntime`]: a
//! start from the launcher (`launcher`), from command-line flags (`main`),
//! a headless replay (`replay`) and the documentation gallery (`gallery`)
//! differ only in the [`SessionConfig`] they build (§2.3 rule 2). After
//! that the same steps run in the same order:
//!
//! 1. [`SessionRuntime::start`] validates the edition (§2.2 rule 3), then
//!    loads what the configuration names: the game flow (`game`), a map
//!    preview (`map_preview`), a movie or a sprite (`preview`).
//! 2. [`SessionRuntime::open_audio`] attaches the output a world or a movie
//!    plays through (a replay or the gallery opens none).
//! 3. Each frame, [`SessionRuntime::advance`] ticks the session, stops a
//!    finished one and hands back the engine key repeats that came due.
//! 4. Each key press goes through [`SessionRuntime::press_key`].
//!
//! What differs by edition is read from one table, [`EditionRules`]: the
//! game flow, the data mount and the edition check branch on its fields,
//! and only [`EditionRules::of`] matches on the profile (§2.2 rule 3).
//!
//! Worlds are configured by one step too: [`WorldOptions::apply`] gives a
//! map preview and each of the game's worlds the session's movie policy,
//! inspector, enhanced presentation, touch UI and mod export.
//!
//! Between those steps the session is its [`AssetPreview`] (the façade
//! dereferences to it), so input routing stays where it is.
use crate::{
    AppResult,
    audio::Audio,
    enhanced::EnhancedSettings,
    input::KeyRepeat,
    map_preview::MapPreview,
    preview::{AssetPreview, PreviewArgs},
};
use std::path::{Path, PathBuf};
use vault_core::GameProfile;
use vault_formats::{Dat2Archive, DatArchive};
use winit::keyboard::KeyCode;

/// Where a session was started from (SPEC-020 §2.3 rule 1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionOrigin {
    /// Command-line flags, or a zero-argument start from the settings.
    CommandLine,
    /// The launcher's Play.
    Launcher,
    /// A headless replay of an input log (`--replay-inputs`, and the
    /// replay tests).
    Replay,
    /// A headless documentation capture (`--capture-gallery`, run by
    /// `cargo xtask capture-gallery`).
    Gallery,
}

/// What a session runs: everything an entry point decides before the
/// shared steps begin.
#[derive(Debug, Clone)]
pub struct SessionConfig {
    pub origin: SessionOrigin,
    /// The asset or game, its data, edition, preset and presentation.
    pub args: PreviewArgs,
}

impl SessionConfig {
    pub fn new(origin: SessionOrigin, args: PreviewArgs) -> Self {
        Self { origin, args }
    }
    /// A replay's session: headless, so its worlds complete movies instead
    /// of waiting for them.
    pub fn replay(args: PreviewArgs) -> Self {
        Self::new(
            SessionOrigin::Replay,
            PreviewArgs {
                headless: true,
                ..args
            },
        )
    }
}

/// A game edition the session cannot run (SPEC-020 §2.2 rule 3: refused at
/// session start, never handled by a default branch).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EditionError {
    /// A name that is neither edition (`--game-profile`).
    Unknown(String),
    /// The configured master database is the other edition's archive.
    Mismatch {
        edition: GameProfile,
        found: GameProfile,
        master: PathBuf,
    },
}

impl std::fmt::Display for EditionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unknown(name) => write!(
                f,
                "unsupported game edition `{name}`: expected fallout1 or fallout2"
            ),
            Self::Mismatch {
                edition,
                found,
                master,
            } => write!(
                f,
                "unsupported game edition for this install: {} is a {} archive, but the \
                 session's edition is {} (set the game to {})",
                master.display(),
                edition_title(*found),
                edition_title(*edition),
                crate::config::game_name(*found),
            ),
        }
    }
}
impl std::error::Error for EditionError {}

/// An edition's name in messages.
fn edition_title(edition: GameProfile) -> &'static str {
    EditionRules::of(edition).title
}

/// How an edition's data is mounted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Archives {
    /// SPEC-008: a MASTER.DAT and CRITTER.DAT pair.
    Dat,
    /// SPEC-015 and SPEC-018 §1-§2: the Fallout 2 install found in the
    /// folder that holds MASTER.DAT, with its optional patch000.dat.
    Dat2Install,
}

/// The session-level differences between the editions outside the world
/// (SPEC-020 §2.2 rule 3). [`EditionRules::of`] is the one place the
/// platform session branches on [`GameProfile`]: the game flow, the data
/// mount and the edition check read these fields rather than comparing
/// profiles at each site. Its `match` has no default arm, so a new edition
/// cannot start without rules of its own.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EditionRules {
    /// The edition these rules are for.
    pub edition: GameProfile,
    /// The other edition, whose archives a mismatched install holds.
    pub other: GameProfile,
    /// The edition's name in messages.
    pub title: &'static str,
    /// How the session mounts the edition's data.
    pub archives: Archives,
    /// SPEC-019 §3.2: the main menu is Fallout 2's six buttons
    /// (`vault_core::ui::fallout2`); otherwise SPEC-014 §3.3-§3.5's.
    pub six_button_menu: bool,
    /// SPEC-019 §3.3: the selection portrait fills Fallout 2's selection
    /// screen window; otherwise SPEC-014's monitor holds it.
    pub portrait_in_screen: bool,
    /// SPEC-014: the premade's ID text under the portrait (SPEC-019 §3.3
    /// shows none).
    pub premade_id: bool,
    /// SPEC-018 §6.6 step 1: the campaign tables and initializer are
    /// parsed before the selection screen, a load or the preferences.
    pub campaign_tables: bool,
}

impl EditionRules {
    /// The rules for `edition`.
    pub const fn of(edition: GameProfile) -> Self {
        match edition {
            GameProfile::Fallout1 => Self {
                edition,
                other: GameProfile::Fallout2,
                title: "Fallout 1",
                archives: Archives::Dat,
                six_button_menu: false,
                portrait_in_screen: false,
                premade_id: true,
                campaign_tables: false,
            },
            GameProfile::Fallout2 => Self {
                edition,
                other: GameProfile::Fallout1,
                title: "Fallout 2",
                archives: Archives::Dat2Install,
                six_button_menu: true,
                portrait_in_screen: true,
                premade_id: false,
                campaign_tables: true,
            },
        }
    }
}

/// An edition named on the command line: `fallout1` or `fallout2`, as the
/// settings' `game` key; any other name is refused.
pub fn parse_edition(text: &str) -> Result<GameProfile, EditionError> {
    crate::config::parse_game(text).ok_or_else(|| EditionError::Unknown(text.to_owned()))
}

/// Whether `master` opens as `edition`'s archive format (SPEC-008 for
/// Fallout 1, SPEC-015 for Fallout 2). Only directory tables are read.
fn opens_as(master: &Path, edition: GameProfile) -> bool {
    match EditionRules::of(edition).archives {
        Archives::Dat => DatArchive::open(master).is_ok(),
        Archives::Dat2Install => Dat2Archive::open(master).is_ok(),
    }
}

/// SPEC-020 §2.2 rule 3: the edition a session starts with, checked against
/// its data. A master database that opens only as the other edition's
/// archive is refused, naming the edition it is. Without a master database
/// (loose data) there is nothing to tell the editions apart here; a map of
/// the other edition is refused by its version when it loads (SPEC-016).
/// A file that is neither archive is left to the load, which reports why.
pub fn validate_edition(
    edition: GameProfile,
    master: Option<&Path>,
) -> Result<GameProfile, EditionError> {
    let other = EditionRules::of(edition).other;
    if let Some(master) = master
        && opens_as(master, other)
        && !opens_as(master, edition)
    {
        return Err(EditionError::Mismatch {
            edition,
            found: other,
            master: master.to_path_buf(),
        });
    }
    Ok(edition)
}

/// What a frame did to the session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Advance {
    /// A standalone movie ended, a map preview's quit was confirmed, or
    /// the game exited: the session is over.
    Finished,
    /// The session runs on: whether its frame changed, and the held keys
    /// whose engine repeat came due (SPEC-014 §3.29), to press in order.
    Running { changed: bool, due: Vec<KeyCode> },
}

/// What a key press did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyPress {
    /// The session took it; its frame changed.
    Changed,
    /// An Escape the session did not take, where Escape exits.
    Exit,
    /// Nothing.
    Ignored,
}

/// The single platform session (SPEC-020 §2.3).
pub struct SessionRuntime {
    origin: SessionOrigin,
    edition: GameProfile,
    preview: AssetPreview,
}

impl SessionRuntime {
    /// Validate the configuration's edition, then load what it names.
    /// `None` when it names nothing to show (the bare color test).
    pub fn start(config: SessionConfig) -> AppResult<Option<Self>> {
        let SessionConfig { origin, args } = config;
        let edition = validate_edition(args.game_profile, args.master_dat.as_deref())?;
        let Some(preview) = args.load()? else {
            return Ok(None);
        };
        tracing::info!(
            ?origin,
            edition = crate::config::game_name(edition),
            "session started"
        );
        Ok(Some(Self {
            origin,
            edition,
            preview,
        }))
    }
    /// The validated edition the session runs.
    pub fn edition(&self) -> GameProfile {
        self.edition
    }
    /// Open the audio output a world or a movie plays through (`device`:
    /// a real output device, else a silent one) and attach it. Sprites need
    /// none, and neither does a replay or the gallery.
    pub fn open_audio(&mut self, device: bool) -> Option<Audio> {
        if matches!(self.origin, SessionOrigin::Replay | SessionOrigin::Gallery) {
            return None;
        }
        let output = || {
            if device {
                Audio::open()
            } else {
                Audio::silent()
            }
        };
        match &mut self.preview {
            AssetPreview::Map(map) => {
                let audio = output();
                map.audio = audio.handle();
                Some(audio)
            }
            AssetPreview::Movie(movie) => {
                let audio = output();
                movie.player.audio = audio.handle();
                Some(audio)
            }
            AssetPreview::Game(game) => {
                let audio = output();
                game.set_audio(audio.handle());
                Some(audio)
            }
            AssetPreview::Sprite(_) => None,
        }
    }
    /// One frame at `now`: the session ticks, a finished one stops, and
    /// the held keys whose repeat came due are returned.
    pub fn advance(&mut self, now: u64, repeat: &mut KeyRepeat) -> AppResult<Advance> {
        let changed = self.preview.tick(now)?;
        if self.preview.finished() {
            return Ok(Advance::Finished);
        }
        let due = repeat.poll(now, self.preview.keyboard());
        Ok(Advance::Running { changed, due })
    }
    /// A key press (or an engine repeat). An Escape the session does not
    /// take exits when `exits` (the standalone previews do; the game never
    /// exits on a key, G-01).
    pub fn press_key(&mut self, key: KeyCode, exits: bool) -> KeyPress {
        if self.preview.step(key) {
            KeyPress::Changed
        } else if key == KeyCode::Escape && exits {
            KeyPress::Exit
        } else {
            KeyPress::Ignored
        }
    }
}

impl std::ops::Deref for SessionRuntime {
    type Target = AssetPreview;
    fn deref(&self) -> &AssetPreview {
        &self.preview
    }
}
impl std::ops::DerefMut for SessionRuntime {
    fn deref_mut(&mut self) -> &mut AssetPreview {
        &mut self.preview
    }
}

/// The session options a world starts with: one step configures a map
/// preview and each world the game creates (SPEC-020 §2.3 rule 2).
pub(crate) struct WorldOptions {
    /// Worlds complete movies instead of waiting for them.
    pub headless: bool,
    /// The vault-debug inspector runs.
    pub debug: bool,
    /// The enhanced presentation (`None`: Vanilla).
    pub enhanced: Option<EnhancedSettings>,
    /// The touch UI's pill bar shows.
    pub touch_ui: bool,
    /// Where the F9 overlay exports the current setup as a mod.
    pub mod_export: Option<PathBuf>,
    /// A `--debug` session (debug-only world actions).
    pub debug_session: bool,
    /// SPEC-022 the session's world-canvas renderer, shared by its worlds.
    pub renderer: crate::map_preview::SharedRenderer,
}

impl WorldOptions {
    /// Apply the options to `world`. `before_debug` runs once the movie
    /// policy is set and before the inspector attaches (a map preview
    /// queues its startup movies there).
    pub fn apply(self, world: &mut MapPreview, before_debug: impl FnOnce(&mut MapPreview)) {
        if self.headless {
            world.host.movie_policy = vault_core::movie::MoviePolicy::CompleteImmediately;
        }
        before_debug(world);
        world.set_debug(self.debug);
        world.set_enhanced(self.enhanced);
        world.set_touch_ui(self.touch_ui);
        world.mod_export = self.mod_export;
        world.debug_session = self.debug_session;
        world.set_scene_renderer(self.renderer);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map_preview::test_support::synthetic_corpus;
    use GameProfile::{Fallout1, Fallout2};

    const MAP: &str = "maps/synthetic.map";

    /// A map session over `master`/`critter` under `edition`.
    fn dat_args(edition: GameProfile, master: &Path, critter: &Path) -> PreviewArgs {
        PreviewArgs {
            game_profile: edition,
            master_dat: Some(master.to_path_buf()),
            critter_dat: Some(critter.to_path_buf()),
            preview_map: Some(MAP.into()),
            ..Default::default()
        }
    }

    #[test]
    fn both_editions_parse_and_any_other_name_is_refused() {
        assert_eq!(parse_edition("fallout1"), Ok(Fallout1));
        assert_eq!(parse_edition("fallout2"), Ok(Fallout2));
        for name in ["fallout3", "tactics", "", "Fallout 2"] {
            let error = parse_edition(name).unwrap_err();
            assert_eq!(error, EditionError::Unknown(name.to_owned()));
            let text = error.to_string();
            assert!(text.contains("unsupported game edition"), "{text}");
            assert!(text.contains("expected fallout1 or fallout2"), "{text}");
        }
    }

    #[test]
    fn each_edition_has_its_own_rules() {
        let fallout1 = EditionRules::of(Fallout1);
        let fallout2 = EditionRules::of(Fallout2);
        assert_eq!((fallout1.edition, fallout1.other), (Fallout1, Fallout2));
        assert_eq!((fallout2.edition, fallout2.other), (Fallout2, Fallout1));
        assert_eq!((fallout1.title, fallout2.title), ("Fallout 1", "Fallout 2"));
        assert_eq!(fallout1.archives, Archives::Dat);
        assert_eq!(fallout2.archives, Archives::Dat2Install);
        // SPEC-014's menu, monitor portrait and ID text; SPEC-019's six
        // buttons, screen portrait and no ID; SPEC-018 §6.6's tables.
        assert!(!fallout1.six_button_menu && fallout2.six_button_menu);
        assert!(!fallout1.portrait_in_screen && fallout2.portrait_in_screen);
        assert!(fallout1.premade_id && !fallout2.premade_id);
        assert!(!fallout1.campaign_tables && fallout2.campaign_tables);
        for edition in crate::config::GAMES {
            let rules = EditionRules::of(edition);
            assert_eq!(EditionRules::of(rules.other).other, edition);
        }
    }

    #[test]
    fn each_edition_accepts_its_own_archives_and_refuses_the_other() {
        let fallout1 = tempfile::tempdir().unwrap();
        vault_fixtures::dat::fallout1_install(fallout1.path());
        let fallout1 = fallout1.path().join("MASTER.DAT");
        let corpus = synthetic_corpus();
        let install = tempfile::tempdir().unwrap();
        let (fallout2, _) =
            vault_fixtures::dat2::fallout2_install(corpus.path(), install.path(), 2);

        assert_eq!(validate_edition(Fallout1, Some(&fallout1)), Ok(Fallout1));
        assert_eq!(validate_edition(Fallout2, Some(&fallout2)), Ok(Fallout2));
        assert_eq!(
            validate_edition(Fallout2, Some(&fallout1)),
            Err(EditionError::Mismatch {
                edition: Fallout2,
                found: Fallout1,
                master: fallout1.clone(),
            })
        );
        assert_eq!(
            validate_edition(Fallout1, Some(&fallout2)),
            Err(EditionError::Mismatch {
                edition: Fallout1,
                found: Fallout2,
                master: fallout2.clone(),
            })
        );
        // Loose data, a missing file or a file that is neither archive is
        // left to the load.
        let junk = install.path().join("junk.dat");
        std::fs::write(&junk, b"not an archive").unwrap();
        let missing = install.path().join("missing.dat");
        for edition in crate::config::GAMES {
            assert_eq!(validate_edition(edition, None), Ok(edition));
            assert_eq!(validate_edition(edition, Some(&junk)), Ok(edition));
            assert_eq!(validate_edition(edition, Some(&missing)), Ok(edition));
        }
    }

    #[test]
    fn both_editions_start_a_session() {
        let corpus = synthetic_corpus();
        let loose = SessionRuntime::start(SessionConfig::replay(PreviewArgs {
            corpus_dir: Some(corpus.path().to_path_buf()),
            preview_map: Some(MAP.into()),
            ..Default::default()
        }))
        .unwrap()
        .unwrap();
        assert_eq!(loose.edition(), Fallout1);
        assert_eq!(loose.origin, SessionOrigin::Replay);
        let AssetPreview::Map(map) = &*loose else {
            panic!("a map session");
        };
        assert_eq!(map.host.world.map.header.version, 19);

        let install = tempfile::tempdir().unwrap();
        let (master, critter) =
            vault_fixtures::dat2::fallout2_install(corpus.path(), install.path(), 2);
        let mut fallout2 = SessionRuntime::start(SessionConfig::new(
            SessionOrigin::CommandLine,
            PreviewArgs {
                headless: true,
                ..dat_args(Fallout2, &master, &critter)
            },
        ))
        .unwrap()
        .unwrap();
        assert_eq!(fallout2.edition(), Fallout2);
        let AssetPreview::Map(map) = &*fallout2 else {
            panic!("a map session");
        };
        assert_eq!(map.host.world.map.header.version, 20);
        // The shared frame step runs the session on.
        let mut repeat = KeyRepeat::default();
        assert!(matches!(
            fallout2.advance(16, &mut repeat).unwrap(),
            Advance::Running { .. }
        ));
    }

    #[test]
    fn a_session_whose_install_is_the_other_edition_is_refused_at_start() {
        let corpus = synthetic_corpus();
        let install = tempfile::tempdir().unwrap();
        let (master, critter) =
            vault_fixtures::dat2::fallout2_install(corpus.path(), install.path(), 2);
        for origin in [
            SessionOrigin::CommandLine,
            SessionOrigin::Launcher,
            SessionOrigin::Replay,
        ] {
            let error = SessionRuntime::start(SessionConfig::new(
                origin,
                dat_args(Fallout1, &master, &critter),
            ))
            .err()
            .expect("refused");
            let text = error.to_string();
            assert!(text.contains("unsupported game edition"), "{text}");
            assert!(text.contains("is a Fallout 2 archive"), "{text}");
            assert!(text.contains("set the game to fallout2"), "{text}");
        }

        let fallout1 = tempfile::tempdir().unwrap();
        vault_fixtures::dat::fallout1_install(fallout1.path());
        let error = SessionRuntime::start(SessionConfig::new(
            SessionOrigin::Launcher,
            PreviewArgs {
                game: true,
                preview_map: None,
                ..dat_args(
                    Fallout2,
                    &fallout1.path().join("MASTER.DAT"),
                    &fallout1.path().join("CRITTER.DAT"),
                )
            },
        ))
        .err()
        .expect("refused");
        let text = error.to_string();
        assert!(text.contains("is a Fallout 1 archive"), "{text}");
        assert!(text.contains("session's edition is Fallout 2"), "{text}");
    }

    #[test]
    fn a_configuration_naming_nothing_starts_no_session() {
        let session = SessionRuntime::start(SessionConfig::new(
            SessionOrigin::CommandLine,
            PreviewArgs::default(),
        ))
        .unwrap();
        assert!(session.is_none());
    }
}
