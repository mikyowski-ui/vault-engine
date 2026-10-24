//! SPEC-014 §3.46 floating text in the interactive world: the background
//! update that removes replaced and expired texts, and each text's box,
//! composed for the map scene's floating-text layer (§3.44).
use super::MapPreview;
use vault_core::{assets::WorldAssets, floating::Ink, ui::ColorKey};
use vault_formats::TextFlags;
use vault_render::{
    TextStyle,
    floating::{FloatingBox, FloatingSprite, compose},
};

impl MapPreview {
    /// One background update of the floating texts at the shell clock
    /// (real time): the preferences' `text_base_delay` times new texts, and
    /// replaced or expired texts go. Removal is checked only while map
    /// background processing runs (not in dialogue, the action menu, a
    /// counter roll or the modal screens, which never reach this update).
    /// Returns whether any text went.
    pub(crate) fn floating_tick(&mut self, now: u64) -> bool {
        let world = &mut self.host.world;
        world.floating.base_delay = self.preferences.text_base_delay;
        if !world.background_enabled || world.dialogue.open {
            return false;
        }
        world.floating.update(now)
    }
    /// The floating texts' boxes, in creation order. A text whose font
    /// cannot load draws nothing.
    pub(crate) fn floating_sprites(&self) -> Vec<FloatingSprite> {
        let resolve = |ink: Ink| match ink {
            Ink::Key(key) => TextStyle {
                foreground: ColorKey(key).resolve(&self.palette),
                ..Default::default()
            },
            Ink::Raw(word) => TextStyle::from(TextFlags::from_word(word)),
        };
        self.host
            .world
            .floating
            .texts
            .iter()
            .filter_map(|text| {
                let font = self.host.assets.font_number(text.font)?;
                let image = compose(
                    &font,
                    &self.palette,
                    FloatingBox {
                        lines: &text.lines,
                        size: text.size,
                        style: resolve(text.color),
                        outline: text.outline.map(|ink| resolve(ink).foreground),
                    },
                )
                .ok()?;
                Some(FloatingSprite {
                    hex: text.hex,
                    offset: text.offset,
                    image,
                })
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use vault_core::{
        assets::{VfsWorldAssets, WorldAssets},
        floating::{layout, lifetime, place},
        ui::ColorKey,
    };
    use vault_formats::{LocalDirProvider, Palette, Vfs};

    /// SPEC-014 §4.21 Fixture U over the corpus `font1.aaf` and
    /// `color.pal`: the widths, boxes, line positions, colors and
    /// placements. Run with `cargo test -p vault-platform -- --ignored`.
    #[test]
    #[ignore = "needs the local corpus"]
    fn spec014_fixture_u_over_the_corpus_font_and_palette() {
        let dir = std::env::var_os("VAULT_CORPUS")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| {
                std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../local-corpus")
            })
            .join("fallout1-unpacked");
        if !dir.is_dir() {
            eprintln!("skipping: no local corpus at {}", dir.display());
            return;
        }
        let vfs: Arc<dyn Vfs> = Arc::new(LocalDirProvider::mount(dir).unwrap());
        let palette = Palette::parse(&vfs.read("color.pal").unwrap()).unwrap();
        let assets = VfsWorldAssets::new(vfs, "english").unwrap();
        let font = assets.font_number(101).expect("font1.aaf");
        let width = |text: &[u8]| font.text_width(text, false) as i32;
        assert_eq!(width(b"Hello stranger. Welcome to"), 160);
        assert_eq!(width(b"Shady Sands."), 80);
        assert_eq!(width(b"Die, scum!"), 58);
        let text = b"Hello stranger. Welcome to Shady Sands.";
        assert_eq!(width(text), 245);
        let two = layout(text, Some(&font), true).unwrap();
        assert_eq!(
            two.lines,
            [
                b"Hello stranger. Welcome to".to_vec(),
                b"Shady Sands.".to_vec()
            ]
        );
        assert_eq!(two.size, [162, 24]);
        // Line 0 at box (1, 1), line 1 at (41, 12).
        let x = |line: &[u8]| (two.size[0] - width(line)) / 2;
        assert_eq!((x(&two.lines[0]), x(&two.lines[1])), (1, 41));
        assert_eq!(place([304, 200], two.size), [239, 116]);
        assert_eq!(place([600, 50], two.size), [422, 10]);
        assert_eq!(lifetime(3.5, two.lines.len()), 5500);
        // Colors: key 32747 [57], key 0 [228]; styles 2, 3, 4.
        let index = |key| ColorKey(key).resolve(&palette);
        assert_eq!((index(32747), index(0)), (57, 228));
        assert_eq!([index(31744), index(992), index(31)], [133, 215, 106]);
        // Taunts: box 60 by 13 with the outline, 58 by 11 without; 4500 ms.
        let taunt = layout(b"Die, scum!", Some(&font), true).unwrap();
        assert_eq!(taunt.size, [60, 13]);
        assert_eq!(
            layout(b"Die, scum!", Some(&font), false).unwrap().size,
            [58, 11]
        );
        assert_eq!(lifetime(3.5, taunt.lines.len()), 4500);
        // Style -2: font 103, box height 21 + 2.
        let large = assets.font_number(103).expect("font3.aaf");
        assert_eq!(layout(b"Warning", Some(&large), true).unwrap().size[1], 23);
    }
}
