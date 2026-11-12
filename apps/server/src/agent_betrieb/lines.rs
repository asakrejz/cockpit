use super::*;

// ---------- Zeilen lesen (begrenzt) ----------

pub enum Gelesen {
    Zeile(String),
    ZuGross,
}

/// Liest eine Zeile, ohne mehr als `MAX_ZEILE` Byte zu puffern: Eine längere Zeile wird bis zum
/// Zeilenende überlesen und als verworfen gemeldet. `None` am Ende des Stroms.
pub async fn lies_zeile<R: AsyncBufRead + Unpin>(
    r: &mut R,
    puffer: &mut Vec<u8>,
) -> std::io::Result<Option<Gelesen>> {
    puffer.clear();
    let mut zu_gross = false;
    let mut gelesen_etwas = false;
    loop {
        let chunk = r.fill_buf().await?;
        if chunk.is_empty() {
            if !gelesen_etwas {
                return Ok(None);
            }
            break;
        }
        gelesen_etwas = true;
        let (nutz, ende) = match chunk.iter().position(|b| *b == b'\n') {
            Some(i) => (&chunk[..i], Some(i + 1)),
            None => (chunk, None),
        };
        if !zu_gross {
            if puffer.len() + nutz.len() > MAX_ZEILE {
                zu_gross = true;
                puffer.clear();
            } else {
                puffer.extend_from_slice(nutz);
            }
        }
        let verbrauch = ende.unwrap_or(chunk.len());
        r.consume(verbrauch);
        if ende.is_some() {
            break;
        }
    }
    Ok(Some(if zu_gross {
        Gelesen::ZuGross
    } else {
        Gelesen::Zeile(String::from_utf8_lossy(puffer).into_owned())
    }))
}
