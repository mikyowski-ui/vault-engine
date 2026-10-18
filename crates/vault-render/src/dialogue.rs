use crate::{ImageError, IndexedImage, TextStyle};
use vault_core::{WorldError, assets::WorldAssets, dialogue::GDialog};
use vault_formats::{Font, Palette};

#[derive(Debug, thiserror::Error)]
pub enum DialogueRenderError {
    #[error(transparent)]
    Image(#[from] ImageError),
    #[error(transparent)]
    Resource(#[from] WorldError),
}
#[derive(Debug, Clone, Copy)]
pub struct ChoiceHit {
    pub index: usize,
    pub rectangle: [i32; 4],
}
impl ChoiceHit {
    pub fn contains(self, point: [i32; 2]) -> bool {
        point[0] >= self.rectangle[0]
            && point[1] >= self.rectangle[1]
            && point[0] < self.rectangle[0] + self.rectangle[2]
            && point[1] < self.rectangle[1] + self.rectangle[3]
    }
}
pub struct DialogueLayout {
    pub image: IndexedImage,
    pub choices: Vec<ChoiceHit>,
    pub page_offsets: Vec<usize>,
}

/// Greedy word wrap at `width` pixels: `(start, end)` byte ranges; an LF or
/// NUL ends a line and a word wider than a line is split.
pub(crate) fn lines(font: &Font, text: &[u8], width: u32) -> Vec<(usize, usize)> {
    let mut result = Vec::new();
    let mut start = 0;
    while start < text.len() {
        let mut end = start;
        let mut measured = 0_u64;
        let mut last_space = None;
        while end < text.len() && text[end] != b'\n' && text[end] != 0 {
            let advance = font.text_width(&text[end..end + 1], false);
            if measured + advance > u64::from(width) {
                break;
            }
            measured += advance;
            if text[end].is_ascii_whitespace() {
                last_space = Some(end);
            }
            end += 1;
        }
        if end < text.len()
            && text[end] != b'\n'
            && text[end] != 0
            && let Some(space) = last_space.filter(|space| *space > start)
        {
            end = space;
        }
        if end == start {
            end = (start + 1).min(text.len());
        }
        result.push((start, end));
        if text.get(end) == Some(&0) {
            break;
        }
        start = end;
        if text
            .get(start)
            .is_some_and(|byte| byte.is_ascii_whitespace())
        {
            start += 1;
        }
    }
    if result.is_empty() {
        result.push((0, 0));
    }
    result
}
fn text_block(
    image: &mut IndexedImage,
    font: &Font,
    palette: &Palette,
    text: &[u8],
    rows: &[(usize, usize)],
    rect: [i32; 4],
    color: u8,
) -> Result<(), ImageError> {
    let mut block = IndexedImage::new(rect[2] as u32, rect[3] as u32)?;
    for y in 0..rect[3] {
        for x in 0..rect[2] {
            let sx = rect[0] + x;
            let sy = rect[1] + y;
            if sx >= 0 && sy >= 0 && sx < image.width() as i32 && sy < image.height() as i32 {
                block.pixels_mut()[(y * rect[2] + x) as usize] =
                    image.pixels()[(sy as u32 * image.width() + sx as u32) as usize];
            }
        }
    }
    for (line, (start, end)) in rows.iter().enumerate() {
        block.draw_text(
            font,
            palette,
            &text[*start..*end],
            [0, (line as u32 * font.line_height()) as i32],
            rect[2] as u32,
            TextStyle {
                foreground: color,
                ..Default::default()
            },
        );
    }
    image.copy_image(&block, [rect[0], rect[1]]);
    Ok(())
}
pub fn compose(
    scene: &IndexedImage,
    dialogue: &GDialog,
    font: &Font,
    palette: &Palette,
    assets: &impl WorldAssets,
    empathy: bool,
    option_scroll: usize,
) -> Result<DialogueLayout, DialogueRenderError> {
    let mut image = IndexedImage::new(640, 480)?;
    image.copy_image(scene, [0, 0]);
    let shell = assets.frame(&assets.interface_path(103)?)?;
    if let Some(frame) = shell.frame(0, 0) {
        image.blit(frame, [0, 0]);
    }
    let panel = assets.frame(&assets.interface_path(99)?)?;
    if let Some(frame) = panel.frame(0, 0) {
        image.blit(frame, [0, 335]);
    }
    if let Some(head) = &dialogue.head {
        let background = assets.frame(&assets.background_path(dialogue.background)?)?;
        if let Some(frame) = background.frame(0, 0) {
            image.blit(frame, [126, 14]);
        }
        // SPEC-006 §3.11 safe rule: missing head art draws no head frame.
        let art = head.path().ok().and_then(|path| assets.frame(&path).ok());
        if let Some(frame) = art.as_ref().and_then(|art| art.frame(0, head.frame)) {
            image.blit(frame, frame.centered_origin([126, 14], [388, 200]));
        }
    }
    let green = palette.lookup_rgb5([0, 31, 0]).unwrap_or(1);
    let reply_lines = lines(font, &dialogue.resolved_reply, 379);
    let per_page = (58 / font.line_height().max(1)).max(1) as usize;
    // Every page of the reply is kept (SPEC-006 §3.10 safe rule), so the
    // remembered-page list never overflows.
    let pages = reply_lines.chunks(per_page).collect::<Vec<_>>();
    let page = dialogue.page.min(pages.len() - 1);
    text_block(
        &mut image,
        font,
        palette,
        &dialogue.resolved_reply,
        pages[page],
        [135, 225, 379, 58],
        green,
    )?;
    let page_offsets = pages.iter().map(|page| page[0].0).collect();
    let mut choices = Vec::new();
    let mut y = 335;
    for (index, text) in dialogue
        .resolved_options
        .iter()
        .enumerate()
        .skip(option_scroll)
    {
        if y >= 452 {
            break;
        }
        let wrapped = lines(font, text, 393);
        let height = (wrapped.len() as u32 * font.line_height())
            .max(font.line_height())
            .min((452 - y) as u32) as i32;
        let color = if empathy {
            match dialogue.options[index].reaction {
                49 => palette.lookup_rgb5([0, 0, 31]).unwrap_or(green),
                51 => palette.lookup_rgb5([31, 0, 0]).unwrap_or(green),
                _ => green,
            }
        } else {
            green
        };
        let rect = [127, y, 393, height];
        text_block(&mut image, font, palette, text, &wrapped, rect, color)?;
        choices.push(ChoiceHit {
            index,
            rectangle: rect,
        });
        y += height + 2;
    }
    image.draw_text(
        font,
        palette,
        b"1-9 choose   0 end node   SPACE page   ESC close",
        [127, 462],
        500,
        TextStyle {
            foreground: green,
            ..Default::default()
        },
    );
    Ok(DialogueLayout {
        image,
        choices,
        page_offsets,
    })
}
