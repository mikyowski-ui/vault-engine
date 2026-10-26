//! The on-load check of the scripts mod layers supply (engine extension):
//! a clean one runs under either profile, an unclean one takes SPEC-006
//! §3.7's cannot-load path with one diagnostic, the verdict is kept for the
//! session, and retail scripts are not checked. All synthetic.
use crate::{
    CharacterSheet, EnginePreset, GameProfile, ScriptEvent, WorldError, WorldHost, WorldState,
    assets::{VfsWorldAssets, WorldAssets},
    fallout2::Fallout2World,
    fixture_state as fixture,
    test_support::{count, gvar, npc_prototype, request, requested},
};
use std::{path::Path, sync::Arc};
use vault_formats::{LayeredVfs, LocalDirProvider, ModLayer, Vfs};

/// A script builder by the global its body counts.
type Builder = fn(i32) -> Vec<u8>;
/// The row a mod registers past the base `scripts.lst`.
const REGISTERED: i32 = 4096;

/// A corpus-shaped script whose `start` body is `body(at)`, `at` being the
/// body's own address (for jump targets): the first request runs it.
fn script(body: impl Fn(i32) -> Vec<u8>) -> Vec<u8> {
    let build = |at: i32| fixture::corpus_program(&[], &[(b"start", fixture::returning(body(at)))]);
    let first = build(0);
    let start = i32::from_be_bytes(first[46 + 24 + 16..46 + 24 + 20].try_into().unwrap());
    // `start` begins with its frame word.
    build(start + 2)
}
fn clean(index: i32) -> Vec<u8> {
    script(|_| count(index))
}
/// A discard on the empty stack after the frame.
fn underflow(index: i32) -> Vec<u8> {
    script(|_| [fixture::instruction(0x801a), count(index)].concat())
}
/// One arm pushes a constant the other does not: no host result to absorb.
fn merge_conflict(index: i32) -> Vec<u8> {
    script(|at| {
        [
            fixture::push(at + 20),
            fixture::push(1),
            fixture::instruction(0x802f),
            fixture::push(5),
            count(index),
        ]
        .concat()
    })
}
/// A reserved Fallout 2 word (SPEC-017 §4.1), outside the Fallout 1 set.
fn reserved(index: i32) -> Vec<u8> {
    script(|_| [fixture::instruction(0x815a), count(index)].concat())
}
/// A word outside every set, jumped over: the model never reaches it, the
/// profile's parse refuses it.
fn unreachable_word(index: i32) -> Vec<u8> {
    script(|at| {
        [
            fixture::push(at + 10),
            fixture::instruction(0x8004),
            fixture::instruction(0x8184),
            count(index),
        ]
        .concat()
    })
}
/// `engine_api_version`, its result discarded.
fn enhanced(index: i32) -> Vec<u8> {
    script(|_| {
        [
            fixture::instruction(0x8200),
            fixture::instruction(0x801a),
            count(index),
        ]
        .concat()
    })
}

/// A base folder with `scripts.lst` row 0 `base.int` (`retail`), and a mod
/// layer `mods/extra` registering `modded.int` (`registered`) as row
/// [`REGISTERED`] and holding `overrides` under `scripts/`.
fn write(dir: &Path, retail: &[u8], registered: &[u8], overrides: &[(&str, &[u8])]) {
    let scripts = dir.join("base/scripts");
    let layer = dir.join("mods/extra");
    std::fs::create_dir_all(&scripts).unwrap();
    std::fs::create_dir_all(layer.join("scripts")).unwrap();
    std::fs::write(scripts.join("scripts.lst"), b"base.int # local_vars=0\n").unwrap();
    std::fs::write(scripts.join("base.int"), retail).unwrap();
    std::fs::write(
        layer.join("vault-mod.toml"),
        format!(
            "name = \"Extra\"\n\n[[scripts]]\nfile = \"modded.int\"\nindex = {REGISTERED}\n\
             local_vars = 0\n"
        ),
    )
    .unwrap();
    std::fs::write(layer.join("scripts/modded.int"), registered).unwrap();
    for (name, bytes) in overrides {
        std::fs::write(layer.join("scripts").join(name), bytes).unwrap();
    }
}
fn mount(dir: &Path) -> Arc<dyn Vfs> {
    let base: Arc<dyn Vfs> = Arc::new(LocalDirProvider::mount(dir.join("base")).unwrap());
    let loose = LocalDirProvider::mount(dir.join("mods/extra")).unwrap();
    let layer = ModLayer::new("mods/extra", Some(loose), None, None);
    Arc::new(LayeredVfs::new(base, vec![layer]))
}
/// A world over `vfs` under `game`, with 16 globals and a critter.
fn host(vfs: Arc<dyn Vfs>, game: GameProfile) -> WorldHost<VfsWorldAssets> {
    let assets = VfsWorldAssets::new(vfs, "english").unwrap();
    let mut world = WorldState::new(CharacterSheet::default(), 1).unwrap();
    world.gvars = Some(vec![0; 16]);
    let mut host = WorldHost::new(world, assets);
    if game == GameProfile::Fallout2 {
        host.set_game_profile(GameProfile::Fallout2);
        host.world.fallout2 = Some(Box::new(Fallout2World::new_game(
            crate::spec017_tests::tables(),
            -1,
        )));
    }
    host
}
/// A new record of script file `file` on a new critter, and its first
/// request: whether a procedure ran (a traced request) and the result.
fn first_request(host: &mut WorldHost<VfsWorldAssets>, file: i32) -> (u32, bool) {
    let me = host.world.spawn(npc_prototype(), 20000, 0).unwrap();
    let locals = host.assets.local_count(file);
    let script = host.world.add_script(me, file, 4, locals).unwrap();
    let before = requested(host, ScriptEvent::Critter).len();
    let result = host
        .dispatch(request(script, ScriptEvent::Critter))
        .unwrap();
    assert!(!result.overridden);
    let ran = requested(host, ScriptEvent::Critter).len() > before;
    assert_eq!(ran, host.world.scripts[&script].initialized);
    (script, ran)
}

#[test]
fn a_clean_registered_mod_script_runs_under_fallout1_and_fallout2() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), &clean(1), &clean(2), &[]);
    for game in [GameProfile::Fallout1, GameProfile::Fallout2] {
        let mut host = host(mount(dir.path()), game);
        // No retail table has the row: the registration answers it.
        assert!(host.assets.registry().scripts.contains_key(&4096));
        let (_, ran) = first_request(&mut host, REGISTERED);
        assert!(ran, "{game:?}");
        assert_eq!(gvar(&host, 2), 1, "{game:?}");
        let (path, program) = host.assets.program(REGISTERED).unwrap();
        assert_eq!(path, "scripts/modded.int");
        assert_eq!(program.words().fallout2(), game == GameProfile::Fallout2);
        assert!(host.assets.take_diagnostics().is_empty(), "{game:?}");
    }
}

#[test]
fn a_clean_mod_override_of_a_retail_script_runs() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), &clean(1), &clean(2), &[("base.int", &clean(3))]);
    for game in [GameProfile::Fallout1, GameProfile::Fallout2] {
        let mut host = host(mount(dir.path()), game);
        assert!(first_request(&mut host, 0).1);
        assert_eq!((gvar(&host, 1), gvar(&host, 3)), (0, 1), "{game:?}");
        assert!(host.assets.take_diagnostics().is_empty());
    }
}

#[test]
fn an_unclean_mod_script_is_refused_with_one_diagnostic_and_runs_nothing() {
    let cases: [(GameProfile, Builder, &str); 6] = [
        (
            GameProfile::Fallout1,
            underflow,
            "1 finding: underflow at word 0x801a x1",
        ),
        (
            GameProfile::Fallout2,
            underflow,
            "1 finding: underflow at word 0x801a x1",
        ),
        (
            GameProfile::Fallout1,
            merge_conflict,
            "1 finding: merge conflict x1",
        ),
        (
            GameProfile::Fallout2,
            reserved,
            "1 finding: reserved word at word 0x815a x1",
        ),
        (
            GameProfile::Fallout1,
            reserved,
            "1 finding: refused word at word 0x815a x1",
        ),
        (
            GameProfile::Fallout2,
            unreachable_word,
            "refused by the word set: INT at byte",
        ),
    ];
    for (game, build, expected) in cases {
        // The registered row, and an override of the retail row.
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), &clean(1), &build(2), &[("base.int", &build(3))]);
        let mut host = host(mount(dir.path()), game);
        for (file, path) in [(REGISTERED, "scripts/modded.int"), (0, "scripts/base.int")] {
            for _ in 0..2 {
                let (_, ran) = first_request(&mut host, file);
                assert!(!ran, "{game:?} {expected}");
            }
            assert!(matches!(
                host.assets.program(file),
                Err(WorldError::ScriptRefused(refused)) if refused == path
            ));
            // One diagnostic for the script, however often it is requested.
            let diagnostics = host.assets.take_diagnostics();
            assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
            let diagnostic = &diagnostics[0];
            assert!(
                diagnostic.starts_with(&format!("{path}: mod script from layer \"mods/extra\"")),
                "{diagnostic}"
            );
            assert!(diagnostic.contains(expected), "{diagnostic}");
            let profile = match game {
                GameProfile::Fallout1 => "(fallout1 table)",
                GameProfile::Fallout2 => "(fallout2 table)",
            };
            assert!(diagnostic.contains(profile), "{diagnostic}");
        }
        // No procedure ran: neither body's count, nor the retail one.
        assert_eq!(host.world.gvars.as_ref().unwrap()[1..4], [0, 0, 0]);
        // A refused launch is a load failure too.
        assert!(matches!(
            host.assets.program_by_name(b"modded.int"),
            Err(WorldError::ScriptRefused(_))
        ));
        assert!(host.assets.take_diagnostics().is_empty());
    }
}

#[test]
fn an_enhanced_range_word_validates_only_under_enhanced() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), &clean(1), &enhanced(2), &[]);
    for game in [GameProfile::Fallout1, GameProfile::Fallout2] {
        let mut host = host(mount(dir.path()), game);
        assert!(!first_request(&mut host, REGISTERED).1);
        let diagnostics = host.assets.take_diagnostics();
        assert_eq!(diagnostics.len(), 1);
        assert!(
            diagnostics[0].contains("refused word at word 0x8200 x1"),
            "{diagnostics:?}"
        );
        // The enhanced preset checks with the enhanced range's rows: the
        // verdict of the other table is not kept.
        host.set_preset(EnginePreset::Enhanced);
        let (_, ran) = first_request(&mut host, REGISTERED);
        assert!(ran, "{game:?}");
        assert_eq!(gvar(&host, 2), 1);
        assert!(host.assets.take_diagnostics().is_empty());
        assert!(host.assets.program(REGISTERED).unwrap().1.enhanced());
    }
}

#[test]
fn the_verdict_is_kept_for_the_session() {
    let dir = tempfile::tempdir().unwrap();
    write(
        dir.path(),
        &clean(1),
        &underflow(2),
        &[("other.int", &clean(4))],
    );
    let mut host = host(mount(dir.path()), GameProfile::Fallout1);
    assert!(!first_request(&mut host, REGISTERED).1);
    assert_eq!(host.assets.take_diagnostics().len(), 1);
    // The file turns clean on disk: the refusal stands, undiagnosed again.
    std::fs::write(dir.path().join("mods/extra/scripts/modded.int"), clean(2)).unwrap();
    assert!(!first_request(&mut host, REGISTERED).1);
    assert!(host.assets.program_by_name(b"MODDED.INT").is_err());
    assert!(host.assets.take_diagnostics().is_empty());
    // A clean verdict keeps its checked program: a launch by name reads
    // the file once.
    let (_, first) = host.assets.program_by_name(b"other.int").unwrap();
    std::fs::write(
        dir.path().join("mods/extra/scripts/other.int"),
        underflow(4),
    )
    .unwrap();
    let (_, again) = host.assets.program_by_name(b"scripts/other.int").unwrap();
    assert_eq!(first.image(), again.image());
    assert!(host.assets.take_diagnostics().is_empty());
    // A new session checks again.
    let mut fresh = self::host(mount(dir.path()), GameProfile::Fallout1);
    assert!(fresh.assets.program_by_name(b"other.int").is_err());
    assert!(first_request(&mut fresh, REGISTERED).1);
}

#[test]
fn retail_scripts_are_not_checked() {
    // The same unclean script from the base folder runs: only mod layers'
    // scripts are checked at load.
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), &merge_conflict(1), &clean(2), &[]);
    for game in [GameProfile::Fallout1, GameProfile::Fallout2] {
        let mut host = host(mount(dir.path()), game);
        assert!(first_request(&mut host, 0).1);
        assert_eq!(gvar(&host, 1), 1);
        assert!(host.assets.take_diagnostics().is_empty());
    }
    // So does a base folder without mod layers.
    let base: Arc<dyn Vfs> = Arc::new(LocalDirProvider::mount(dir.path().join("base")).unwrap());
    let mut host = host(base, GameProfile::Fallout1);
    assert!(first_request(&mut host, 0).1);
    assert!(host.assets.take_diagnostics().is_empty());
}
