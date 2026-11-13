use super::*;

// ---------- Zeilen lesen (begrenzt) ----------

pub enum Read {
    Line(String),
    TooLarge,
}

/// Liest eine Zeile, ohne mehr als `MAX_ZEILE` Byte zu puffern: Eine längere Zeile wird bis zum
/// Zeilenende überlesen und als verworfen gemeldet. `None` am Ende des Stroms.
pub async fn read_line<R: AsyncBufRead + Unpin>(
    r: &mut R,
    buffer: &mut Vec<u8>,
) -> std::io::Result<Option<Read>> {
    buffer.clear();
    let mut too_large = false;
    let mut read_something = false;
    loop {
        let chunk = r.fill_buf().await?;
        if chunk.is_empty() {
            if !read_something {
                return Ok(None);
            }
            break;
        }
        read_something = true;
        let (usable, end) = match chunk.iter().position(|b| *b == b'\n') {
            Some(i) => (&chunk[..i], Some(i + 1)),
            None => (chunk, None),
        };
        if !too_large {
            if buffer.len() + usable.len() > MAX_LINE {
                too_large = true;
                buffer.clear();
            } else {
                buffer.extend_from_slice(usable);
            }
        }
        let usage = end.unwrap_or(chunk.len());
        r.consume(usage);
        if end.is_some() {
            break;
        }
    }
    Ok(Some(if too_large {
        Read::TooLarge
    } else {
        Read::Line(String::from_utf8_lossy(buffer).into_owned())
    }))
}
