//! Native image display through the Kitty graphics protocol.

use image::DynamicImage;
use std::io::{self, Write};

const IMAGE_ID: u32 = 731;

pub fn supported() -> bool {
    std::env::var_os("KITTY_WINDOW_ID").is_some()
        || std::env::var("TERM")
            .map(|term| term.contains("kitty"))
            .unwrap_or(false)
}

pub fn show(image: &DynamicImage, width: u16, height: u16) -> io::Result<()> {
    // Match the image viewer's centered dialog and leave its bottom row for the
    // close hint. Kitty scales the original pixels to this cell rectangle.
    let dialog_w = width.saturating_sub(4).min(width);
    let dialog_h = height.saturating_sub(3).min(height);
    let inner_w = dialog_w.saturating_sub(2);
    let inner_h = dialog_h.saturating_sub(3);
    if inner_w == 0 || inner_h == 0 {
        return Ok(());
    }
    let x = (width - dialog_w) / 2 + 1;
    let y = (height - dialog_h) / 2 + 1;
    let rgba = image.to_rgba8();
    let mut out = io::stdout().lock();
    // Replace a previous placement before retransmitting (for example after a
    // terminal resize), then send full-resolution RGBA pixels in Kitty chunks.
    write!(out, "\x1b_Ga=d,d=i,i={IMAGE_ID}\x1b\\")?;
    let bytes = rgba.as_raw();
    let encoded = base64(bytes);
    for (index, chunk) in encoded.as_bytes().chunks(4096).enumerate() {
        let more = usize::from((index + 1) * 4096 < encoded.len());
        if index == 0 {
            write!(
                out,
                "\x1b_Ga=t,f=32,s={},v={},i={},m={};",
                image.width(),
                image.height(),
                IMAGE_ID,
                more
            )?;
        } else {
            write!(out, "\x1b_Gm={more};")?;
        }
        out.write_all(chunk)?;
        write!(out, "\x1b\\")?;
    }
    // Save and restore the cursor: Ratatui tracks its own cursor location and
    // assumes direct terminal writes leave that location alone.
    write!(
        out,
        "\x1b7\x1b[{y};{x}H\x1b_Ga=p,i={IMAGE_ID},c={inner_w},r={inner_h},q=2\x1b\\\x1b8"
    )?;
    out.flush()
}

pub fn clear() -> io::Result<()> {
    let mut out = io::stdout().lock();
    write!(out, "\x1b_Ga=d,d=i,i={IMAGE_ID}\x1b\\")?;
    out.flush()
}

fn base64(input: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut result = String::with_capacity(input.len().div_ceil(3) * 4);
    for chunk in input.chunks(3) {
        let a = chunk[0];
        let b = *chunk.get(1).unwrap_or(&0);
        let c = *chunk.get(2).unwrap_or(&0);
        result.push(TABLE[(a >> 2) as usize] as char);
        result.push(TABLE[(((a & 3) << 4) | (b >> 4)) as usize] as char);
        result.push(if chunk.len() > 1 {
            TABLE[(((b & 15) << 2) | (c >> 6)) as usize] as char
        } else {
            '='
        });
        result.push(if chunk.len() > 2 {
            TABLE[(c & 63) as usize] as char
        } else {
            '='
        });
    }
    result
}
