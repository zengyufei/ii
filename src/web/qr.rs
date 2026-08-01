use anyhow::Result;
use qrcodegen::{QrCode, QrCodeEcc};

pub(crate) fn svg(url: &str) -> Result<String> {
    const QUIET_ZONE: i32 = 4;
    let code = QrCode::encode_text(url, QrCodeEcc::Low)
        .map_err(|_| anyhow::anyhow!("generate web QR code: URL is too long"))?;
    let size = code.size();
    let view_box = size + QUIET_ZONE * 2;
    let mut path = String::new();
    for y in 0..size {
        for x in 0..size {
            if code.get_module(x, y) {
                path.push_str(&format!("M{} {}h1v1h-1z", x + QUIET_ZONE, y + QUIET_ZONE));
            }
        }
    }
    Ok(format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 {view_box} {view_box}\" width=\"240\" height=\"240\" role=\"img\" aria-label=\"QR code\"><rect width=\"100%\" height=\"100%\" fill=\"white\"/><path d=\"{path}\" fill=\"black\"/></svg>"
    ))
}

pub(crate) fn terminal(url: &str) -> Result<String> {
    const QUIET_ZONE: i32 = 4;
    let code = QrCode::encode_text(url, QrCodeEcc::Low)
        .map_err(|_| anyhow::anyhow!("generate web QR code: URL is too long"))?;
    let size = code.size();
    let width = size + QUIET_ZONE * 2;
    let height = width + width.rem_euclid(2);
    let mut output = String::new();
    for y in (0..height).step_by(2) {
        for x in 0..width {
            let top = module(&code, x, y, QUIET_ZONE);
            let bottom = module(&code, x, y + 1, QUIET_ZONE);
            let cell = match (top, bottom) {
                (true, true) => "█",
                (true, false) => "▀",
                (false, true) => "▄",
                (false, false) => " ",
            };
            output.push_str(cell);
        }
        output.push('\n');
    }
    Ok(output)
}

fn module(code: &QrCode, x: i32, y: i32, quiet_zone: i32) -> bool {
    let size = code.size();
    let x = x - quiet_zone;
    let y = y - quiet_zone;
    x >= 0 && x < size && y >= 0 && y < size && code.get_module(x, y)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn svg_is_self_contained_and_deterministic() {
        let url = "http://192.168.1.2:3456/";
        let first = svg(url).unwrap();
        assert_eq!(first, svg(url).unwrap());
        assert!(first.starts_with("<svg "));
        assert!(first.contains("viewBox=\"0 0 "));
        assert!(first.contains("<path d=\"M"));
        assert!(!first.contains(url));
        assert!(!first.contains("href="));
        assert!(!first.contains("<script"));
        assert!(!first.contains("<image"));
        assert!(!first.contains("<foreignObject"));
    }

    #[test]
    fn terminal_is_self_contained_and_deterministic() {
        let url = "http://192.168.1.2:3456/";
        let first = terminal(url).unwrap();
        assert_eq!(first, terminal(url).unwrap());
        assert!(!first.contains(url));
        assert!(first.ends_with('\n'));
        assert!(first.contains('\u{2588}'));
        assert!(first.contains(' '));
        assert!(first.chars().all(|character| matches!(
            character,
            '\u{2588}' | '\u{2580}' | '\u{2584}' | ' ' | '\n'
        )));
    }
}
