use super::{Cell, LayoutGeneration, Size, WindowFrame, cell_encoding};
use serde::{Deserialize, Deserializer, Serialize, de};
use std::collections::HashMap;
use std::io;
use std::sync::Arc;

const BAND_WIDTH: usize = 32;
const MAX_COPY_RUNS: usize = cell_encoding::MAX_CELLS / BAND_WIDTH;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct CellReuse {
    base: u64,
    #[serde(deserialize_with = "deserialize_copies")]
    copies: Vec<CopyRun>,
}

/// Destination offset, source offset, and length in row-major cells.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
struct CopyRun(usize, usize, usize);

/// One connection direction's most recent complete window cell grid.
#[derive(Default)]
pub struct CellFrameCache(Option<CellBase>);

struct CellBase {
    sequence: u64,
    layout: LayoutGeneration,
    size: Size,
    cells: Arc<Vec<Cell>>,
}

impl CellBase {
    fn from_frame(frame: &WindowFrame) -> Self {
        Self {
            sequence: frame.sequence,
            layout: frame.layout,
            size: frame.size,
            cells: frame.cells.clone(),
        }
    }

    fn matches(&self, frame: &WindowFrame) -> bool {
        frame.layout == self.layout && frame.size == self.size && frame.sequence > self.sequence
    }
}

impl CellFrameCache {
    /// Prepare the next outgoing frame after any queue coalescing and retain its complete cells.
    pub fn prepare(&mut self, frame: &mut WindowFrame) {
        let next = CellBase::from_frame(frame);
        if let Some(base) = self.0.as_ref().filter(|base| base.matches(frame))
            && let Some(copies) = find_copies(&base.cells, &frame.cells, frame.size)
        {
            let mut changed = Vec::new();
            let mut start = 0;
            for &CopyRun(target, _, count) in &copies {
                changed.extend_from_slice(&frame.cells[start..target]);
                start = target + count;
            }
            changed.extend_from_slice(&frame.cells[start..]);
            frame.cells = Arc::new(changed);
            frame.cell_reuse = Some(CellReuse {
                base: base.sequence,
                copies,
            });
        }
        self.0 = Some(next);
    }

    /// Restore and validate each received frame in wire order, even if it will not be displayed.
    pub fn restore(&mut self, frame: &mut WindowFrame) -> io::Result<()> {
        if let Some(reuse) = frame.cell_reuse.take() {
            let base = self
                .0
                .as_ref()
                .filter(|base| base.matches(frame) && base.sequence == reuse.base)
                .ok_or_else(invalid_reuse)?;
            frame.cells = restore_cells(base, std::mem::take(&mut frame.cells), &reuse.copies)?;
        }
        frame.validate()?;
        self.0 = Some(CellBase::from_frame(frame));
        Ok(())
    }
}

fn find_copies(previous: &[Cell], current: &[Cell], size: Size) -> Option<Vec<CopyRun>> {
    let width = usize::from(size.width);
    let area = size.area();
    if area < BAND_WIDTH * 8
        || area > cell_encoding::MAX_CELLS
        || previous.len() != area
        || current.len() != area
    {
        return None;
    }

    let mut bands = None;
    let mut copies: Vec<CopyRun> = Vec::new();
    let mut reused = 0;
    let min_reuse = area.div_ceil(2);
    for (row, cells) in current.chunks_exact(width).enumerate() {
        for (band, cells) in cells.chunks(BAND_WIDTH).enumerate() {
            let target = row * width + band * BAND_WIDTH;
            let len = cells.len();
            // Prefer the same run when fixed cells also match at the scroll offset.
            let continued = copies.last().and_then(|&CopyRun(start, source, count)| {
                let source = source + count;
                (start + count == target && previous.get(source..source + len) == Some(cells))
                    .then_some(source)
            });
            let source = continued.or_else(|| {
                if &previous[target..target + len] == cells {
                    Some(target)
                } else {
                    let bands = bands.get_or_insert_with(|| {
                        let mut indices = HashMap::new();
                        for (row, cells) in previous.chunks_exact(width).enumerate() {
                            for (band, cells) in cells.chunks(BAND_WIDTH).enumerate() {
                                indices
                                    .entry((band, cells))
                                    .or_insert(row * width + band * BAND_WIDTH);
                            }
                        }
                        indices
                    });
                    bands.get(&(band, cells)).copied()
                }
            });
            let Some(source) = source else {
                if reused + area - target - len < min_reuse {
                    return None;
                }
                continue;
            };
            reused += len;
            if let Some(CopyRun(start, old_start, count)) = copies.last_mut()
                && *start + *count == target
                && *old_start + *count == source
            {
                *count += len;
            } else if copies.len() == MAX_COPY_RUNS {
                return None;
            } else {
                copies.push(CopyRun(target, source, len));
            }
        }
    }
    // Full compression is cheaper when reuse is sparse or broken into small runs.
    (reused >= min_reuse && reused >= copies.len() * BAND_WIDTH * 2).then_some(copies)
}

fn restore_cells(
    base: &CellBase,
    changed: Arc<Vec<Cell>>,
    copies: &[CopyRun],
) -> io::Result<Arc<Vec<Cell>>> {
    let area = base.cells.len();
    if let [CopyRun(0, 0, len)] = copies
        && *len == area
        && changed.is_empty()
    {
        return Ok(base.cells.clone());
    }

    let mut symbol_bytes: usize = changed.iter().map(|cell| cell.symbol.len()).sum();
    let mut changed = Arc::unwrap_or_clone(changed).into_iter();
    let mut cells = Vec::with_capacity(area);
    for &CopyRun(target, source, len) in copies {
        if len == 0
            || target < cells.len()
            || target.checked_add(len).is_none_or(|end| end > area)
            || source.checked_add(len).is_none_or(|end| end > area)
        {
            return Err(invalid_reuse());
        }
        let reused = &base.cells[source..source + len];
        symbol_bytes += reused.iter().map(|cell| cell.symbol.len()).sum::<usize>();
        if symbol_bytes > cell_encoding::MAX_CELL_BYTES {
            return Err(invalid_reuse());
        }
        cells.extend(changed.by_ref().take(target - cells.len()));
        if cells.len() != target {
            return Err(invalid_reuse());
        }
        cells.extend_from_slice(reused);
    }
    if changed.len() != area - cells.len() {
        return Err(invalid_reuse());
    }
    cells.extend(changed);
    Ok(Arc::new(cells))
}

fn invalid_reuse() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, "invalid window cell reuse")
}

fn deserialize_copies<'de, D: Deserializer<'de>>(de: D) -> Result<Vec<CopyRun>, D::Error> {
    struct Copies;

    impl<'de> de::Visitor<'de> for Copies {
        type Value = Vec<CopyRun>;

        fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("window cell copy runs")
        }

        fn visit_seq<A: de::SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
            let mut copies = Vec::new();
            while let Some(copy) = seq.next_element()? {
                if copies.len() == MAX_COPY_RUNS {
                    return Err(de::Error::custom("too many window cell copy runs"));
                }
                copies.push(copy);
            }
            Ok(copies)
        }
    }

    de.deserialize_seq(Copies)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Color, Cursor, CursorShape, MAX_WINDOW_FRAME_BYTES, transport::encode_message};

    fn viewport(sequence: u64, scroll: usize) -> WindowFrame {
        let size = Size::new(215, 35);
        let mut frame = WindowFrame::new(LayoutGeneration(1), size);
        frame.sequence = sequence;
        frame.processed_input_serial = sequence * 4;
        frame.confirmed_input_fence = sequence * 3;
        frame.cursor = Some(Cursor {
            x: 10,
            y: sequence as u16,
            shape: CursorShape::SteadyBar,
        });
        let cells = Arc::make_mut(&mut frame.cells);
        for (y, row) in cells.chunks_exact_mut(size.width.into()).enumerate() {
            let fixed = y == 0 || y == usize::from(size.height) - 1;
            for (x, cell) in row.iter_mut().enumerate() {
                cell.background = Color::Rgb(7, 17, 22);
                if fixed || x < BAND_WIDTH {
                    cell.symbol = if (x + y) % 3 == 0 { "=" } else { " " }.into();
                } else if x < 192 {
                    let text_row = y + scroll;
                    cell.symbol = match x {
                        63 => "中".into(),
                        64 => " ".into(),
                        83 => "e\u{301}".into(),
                        _ => char::from(b'a' + ((x ^ (text_row * 17)) % 26) as u8)
                            .to_string()
                            .into(),
                    };
                    cell.skip = x == 64;
                    cell.foreground = Color::Indexed(((x + text_row) % 16) as u8);
                    cell.modifiers = (text_row % 3) as u16;
                } else if x == 214 {
                    cell.symbol = if y == scroll + 1 { "▐" } else { "│" }.into();
                }
            }
        }
        frame
    }

    #[test]
    fn scrolling_reuses_shifted_text_bands_without_moving_fixed_controls() {
        let mut first = viewport(1, 0);
        let current = viewport(2, 3);
        let mut writer = CellFrameCache::default();
        let mut reader = CellFrameCache::default();
        writer.prepare(&mut first);
        reader.restore(&mut first).unwrap();

        let mut wire = current.clone();
        writer.prepare(&mut wire);
        let copies = &wire.cell_reuse.as_ref().unwrap().copies;
        assert!(copies.iter().any(|&CopyRun(to, from, _)| to != from));
        assert!(copies.iter().any(|&CopyRun(to, from, _)| to == from));
        assert!(wire.cells.len() < current.cells.len() / 5);
        let full = encode_message(&current, MAX_WINDOW_FRAME_BYTES).unwrap();
        let delta = encode_message(&wire, MAX_WINDOW_FRAME_BYTES).unwrap();
        println!(
            "scroll frame: {} bytes vs {} bytes",
            delta.len(),
            full.len()
        );
        assert!(delta.len() < full.len() / 2);

        let mut received: WindowFrame = serde_json::from_slice(&delta[4..]).unwrap();
        assert!(received.validate().is_err());
        reader.restore(&mut received).unwrap();
        assert_eq!(received, current);
    }

    #[test]
    fn plain_text_scrolling_keeps_margin_cells_with_the_shifted_rows() {
        let size = Size::new(118, 32);
        let make_frame = |scroll: usize| {
            let mut frame = WindowFrame::new(LayoutGeneration(1), size);
            frame.sequence = scroll as u64 + 1;
            for (y, row) in Arc::make_mut(&mut frame.cells)
                .chunks_exact_mut(size.width.into())
                .enumerate()
            {
                let text = match y {
                    0 | 31 => "File | Save | Undo | Find".to_owned(),
                    _ => format!(
                        "{:05}  The document row wraps at a fixed column",
                        y + scroll
                    ),
                };
                for cell in row.iter_mut() {
                    cell.foreground = Color::Indexed(7);
                    cell.background = Color::Rgb(7, 17, 22);
                }
                for (cell, ch) in row.iter_mut().zip(text.chars()) {
                    cell.symbol = ch.to_string().into();
                }
                row.last_mut().unwrap().symbol = if y == scroll + 1 { "▐" } else { "│" }.into();
            }
            frame
        };
        let mut first = make_frame(0);
        let current = make_frame(1);
        let mut wire = current.clone();
        let mut writer = CellFrameCache::default();
        writer.prepare(&mut first);
        writer.prepare(&mut wire);
        let full = encode_message(&current, MAX_WINDOW_FRAME_BYTES).unwrap();
        let delta = encode_message(&wire, MAX_WINDOW_FRAME_BYTES).unwrap();
        println!(
            "plain scroll: {} bytes vs {} bytes",
            delta.len(),
            full.len()
        );
        assert!(delta.len() < full.len() / 2);
        let mut reader = CellFrameCache::default();
        reader.restore(&mut first).unwrap();
        reader.restore(&mut wire).unwrap();
        assert_eq!(wire, current);
    }

    #[test]
    fn unchanged_cells_sharing_does_not_omit_new_cursor_or_input_receipt() {
        let mut first = viewport(1, 0);
        let mut next = viewport(2, 0);
        let mut writer = CellFrameCache::default();
        writer.prepare(&mut first);
        writer.prepare(&mut next);
        assert!(next.cells.is_empty());
        let mut reader = CellFrameCache::default();
        reader.restore(&mut first).unwrap();
        reader.restore(&mut next).unwrap();
        assert!(Arc::ptr_eq(&first.cells, &next.cells));
        assert_ne!(first.cursor, next.cursor);
        assert_eq!(next.processed_input_serial, 8);
        assert_eq!(next.confirmed_input_fence, 6);
    }

    #[test]
    fn layout_size_and_whole_viewport_changes_use_complete_frames() {
        let mut writer = CellFrameCache::default();
        writer.prepare(&mut viewport(1, 0));
        let mut moved = viewport(2, 1);
        moved.layout = LayoutGeneration(2);
        writer.prepare(&mut moved);
        assert!(moved.cell_reuse.is_none());

        let mut resized = WindowFrame::new(moved.layout, Size::new(216, 35));
        resized.sequence = 3;
        writer.prepare(&mut resized);
        assert!(resized.cell_reuse.is_none());

        let mut changed = resized.clone();
        changed.sequence = 4;
        for cell in Arc::make_mut(&mut changed.cells) {
            cell.background = Color::Indexed(7);
        }
        writer.prepare(&mut changed);
        assert!(changed.cell_reuse.is_none());
    }

    #[test]
    fn invalid_base_and_copy_ranges_cannot_reconstruct_a_frame() {
        let mut first = WindowFrame::new(LayoutGeneration(1), Size::new(4, 2));
        first.sequence = 1;
        let mut patch = first.clone();
        patch.sequence = 2;
        patch.cells = Arc::default();
        patch.cell_reuse = Some(CellReuse {
            base: 1,
            copies: vec![CopyRun(0, 0, 8)],
        });
        let mut reader = CellFrameCache::default();
        assert!(reader.restore(&mut patch.clone()).is_err());
        reader.restore(&mut first).unwrap();

        let mut wrong_base = patch.clone();
        wrong_base.cell_reuse.as_mut().unwrap().base = 0;
        assert!(reader.restore(&mut wrong_base).is_err());
        let mut wrong_layout = patch.clone();
        wrong_layout.layout = LayoutGeneration(2);
        assert!(reader.restore(&mut wrong_layout).is_err());
        for copies in [
            vec![CopyRun(0, 0, 1), CopyRun(0, 1, 7)],
            vec![CopyRun(0, 1, 8)],
            vec![CopyRun(usize::MAX, 0, 1)],
            vec![CopyRun(0, 0, 0)],
            vec![CopyRun(0, 0, 7)],
            vec![CopyRun(1, 0, 7)],
        ] {
            let mut invalid = patch.clone();
            invalid.cell_reuse.as_mut().unwrap().copies = copies;
            assert!(reader.restore(&mut invalid).is_err());
        }
        reader.restore(&mut patch).unwrap();
        assert_eq!(patch.cells, first.cells);
    }

    #[test]
    fn copy_count_and_reconstructed_symbol_bytes_are_bounded() {
        let reuse = CellReuse {
            base: 1,
            copies: vec![CopyRun(0, 0, 1); MAX_COPY_RUNS + 1],
        };
        assert!(serde_json::from_slice::<CellReuse>(&serde_json::to_vec(&reuse).unwrap()).is_err());

        let mut first = WindowFrame::new(LayoutGeneration(1), Size::new(4, 1));
        first.sequence = 1;
        Arc::make_mut(&mut first.cells)[0].symbol =
            "x".repeat(cell_encoding::MAX_CELL_BYTES / 3 + 1).into();
        let mut patch = first.clone();
        patch.sequence = 2;
        patch.cells = Arc::default();
        patch.cell_reuse = Some(CellReuse {
            base: 1,
            copies: (0..4).map(|target| CopyRun(target, 0, 1)).collect(),
        });
        let mut reader = CellFrameCache::default();
        reader.restore(&mut first).unwrap();
        assert!(reader.restore(&mut patch).is_err());
    }
}
